# DOM and Layout Colocation Plan

## Status

Draft migration plan for replacing Burokku's cross-thread DOM publication model with a UI-thread-owned live DOM driven by the patched Tokio/native event-loop integration.

## Summary

Layout already runs on the UI thread inside `ApplicationHost`; the mutable DOM and application JavaScript currently run in the background runtime. The patched Tokio integration allows the main QuickJS driver to progress inside the native event loop, so the DOM-facing JavaScript runtime can move onto the UI thread beside window management, layout, scene construction, and presentation.

The recommended final architecture is one JavaScript runtime for the Burokku application. If a background JavaScript isolate is still required, it should be an optional worker that exchanges owned messages with the UI runtime and cannot directly mutate the DOM.

Keeping DOM mutation on the background thread while removing snapshot or message transport is not safe.

```text
AppKit event loop
  -> patched Tokio bounded tick
  -> main QuickJS macrotask and ready microtasks
  -> DOM checkpoint
  -> about_to_wait observes the DOM revision
  -> native-window reconciliation
  -> redraw: borrow DOM -> layout -> build scene and hit-test plan
  -> release DOM borrow
  -> GPU presentation
```

## Goals

1. Run DOM-facing JavaScript, DOM mutation, layout, and scene construction on the UI thread.
2. Replace immutable snapshot publication with controlled borrowing of one live DOM.
3. Preserve macrotask/microtask batch boundaries and revision-based consistency.
4. Make the native event loop the outer owner of the process main thread.
5. Reduce Burokku's application path from two JavaScript runtimes to one unless a separate worker is explicitly requested.
6. Preserve stale-result checks for asynchronous GPU work and generation-checked node handles.

## Non-goals

- Removing DOM revisions or `NodeId` generations.
- Allowing a background isolate to borrow or synchronously mutate the UI DOM.
- Holding a DOM borrow across an `.await`, native operation that may reenter the event loop, or GPU presentation.
- Removing the owned presented-frame hit-test plan.
- Replacing Tokio's bounded external tick with synchronous polling from window callbacks.

## Current architecture

### DOM mutation and publication

`crates/burokku/src/ui/dom_plugin.rs` owns a mutable staging `Dom` in `DomPluginState`. JavaScript bindings access it through `SharedDomState = Arc<Mutex<DomPluginState>>`. `DomPlugin::install` currently rejects `RuntimeRole::Main`, forcing the DOM facade into the background isolate.

After each JavaScript macrotask and its ready microtasks, `DomPlugin::checkpoint`:

1. reclaims detached nodes;
2. asks `DomPublisher` to clone a new immutable DOM snapshot;
3. stores a `PublishedDom` in `ArcSwap`;
4. notifies the native event loop.

The complete publication implementation is in `crates/burokku/src/ui/elements/publication.rs`.

### UI consumption

`ApplicationHost` owns a `PublishedDomReader` and an `Option<Arc<PublishedDom>>`. Its `about_to_wait` callback loads the newest publication, reconciles the native window, completes pending graphics initialization, and checks for another publication.

A redraw clones the retained publication and sends it through:

```text
LayoutEngine::compute
  -> reconcile_full
  -> compute_layout
  -> ComputedLayout
  -> BuiltScene::build
  -> ScenePlan
  -> WindowRenderer::present
```

`ComputedLayout` retains `Arc<PublishedDom>` because scene construction later reads DOM iteration order and paint styles from the publication's snapshot.

### Application runner mismatch

`crates/burokku/src/app.rs` still assumes an already-running Tokio runtime and calls `EventLoop::run_app`. The updated window crate exposes `EventLoop::run_app_external`, which owns a patched current-thread Tokio runtime and one persistent `LocalSet` while the native platform loop remains outermost.

## Target architecture

### Runtime ownership

`Burokku::run` should be a synchronous, process-main-thread entry point:

```rust
pub fn run(self) -> Result<(), BurokkuError> {
    let mut event_loop = winit::EventLoop::new()?;
    let runtime = event_loop
        .external_runtime_builder()
        .enable_all()
        .external_tick_budget(64)
        .build()?;
    let local_set = tokio::task::LocalSet::new();

    // Spawn main QuickJS bootstrap and its driven runtime on `local_set`.

    let host = ApplicationHost::new(shared_dom, text, lifecycle);
    let host = event_loop.run_app_external(host, runtime, local_set)?;
    host.result()
}
```

The persistent `LocalSet` owns the main QuickJS driver and other thread-affine futures. Tokio I/O readiness may be collected by its reactor thread, but application futures, QuickJS, DOM access, layout, and rendering remain on the UI thread.

### DOM ownership

The smallest migration can retain a shared state compatible with the current `Plugin: Send` contract:

```rust
struct UiDomState {
    dom: Dom,
    live_wrappers: HashMap<NodeId, usize>,
    last_reclaim: ReclaimReport,
}

type SharedUiDom = Arc<Mutex<UiDomState>>;
```

This mutex is no longer a cross-thread synchronization mechanism. It is only a compatibility handle between the runtime plugin and `ApplicationHost`. No lock may be held across `.await`, native window operations, or presentation.

A later runtime cleanup may introduce a non-`Send` `LocalPlugin` capability and replace this handle with `Rc<RefCell<UiDomState>>`. That should be a separate migration and should not block snapshot removal.

### Frame transaction

A frame must materialize all DOM-dependent data under one uninterrupted immutable DOM borrow:

1. read `dom.revision()`;
2. compute or reuse layout for that revision, viewport, and text generation;
3. build a fully owned Vello scene and `ScenePlan` from the same DOM revision;
4. assert that the DOM revision has not changed;
5. release the DOM borrow;
6. present the built scene.

The retained `ComputedLayout`, `BuiltScene`, `ScenePlan`, and `PresentedFrame` must not contain Rust references into `Dom`.

### Checkpoint scheduling

Same-thread ownership does not by itself preserve atomic updates. Burokku must continue using the runtime checkpoint after one macrotask and all ready QuickJS microtasks.

DOM mutations may increment the live DOM revision immediately so JavaScript can read its own writes. The native host only reacts after the Tokio tick returns and `about_to_wait` runs. Several completed JavaScript tasks may be coalesced into one native reconciliation and redraw, but layout must never run in the middle of one macrotask/microtask checkpoint.

Native callbacks that dispatch JavaScript events must enqueue a macrotask. They must not synchronously poll the JavaScript runtime from inside the callback.

## 1. Code to remove

### Snapshot and publication module

Delete `crates/burokku/src/ui/elements/publication.rs`, including:

- `ChangeSet`;
- `DomSnapshot`;
- `PublishedDom`;
- `CommitNotifier`;
- `PublishedDomReader`;
- `DomPublisher`;
- `ArcSwap<PublishedDom>`;
- publication-specific unit tests.

Remove `arc-swap` from `crates/burokku/Cargo.toml`.

### Snapshot-oriented DOM storage

In `crates/burokku/src/ui/elements.rs`:

- change `SlotMap<NodeId, Arc<Node>>` to `SlotMap<NodeId, Node>`;
- replace `Arc::make_mut` with ordinary mutable slot access;
- remove `Dom: Clone` if no remaining caller requires it;
- remove comments and tests describing snapshot clones and copy-on-write nodes.

Keep the DOM revision, per-node revisions, stable slotmap keys, and generation checking.

### Publication state in `ApplicationHost`

Remove from `crates/burokku/src/ui/host.rs`:

- `publications: PublishedDomReader`;
- `latest_publication: Option<Arc<PublishedDom>>`;
- `sync_publication`;
- repeated publication loads;
- atomic-publication coalescing logic.

Replace them with a live DOM handle and revision bookkeeping such as:

```rust
dom: SharedUiDom,
observed_revision: u64,
```

### Publication wrappers in consumers

Replace publication-specific signatures:

```text
WindowSpec::from_publication(&PublishedDom)
    -> WindowSpec::from_dom(&Dom)

WindowManager::reconcile(..., &PublishedDom)
    -> WindowManager::reconcile(..., WindowSpec)

LayoutEngine::compute(Arc<PublishedDom>, viewport)
    -> LayoutEngine::compute(&Dom, viewport)

reconcile_full(&PublishedDom, viewport)
    -> reconcile_full(&Dom, viewport)

collect_paragraph(&DomSnapshot, source)
    -> collect_paragraph(&Dom, source)
```

Remove `ComputedLayout::publication: Arc<PublishedDom>` and its `publication()` accessor.

Remove `LayoutError::PublicationRevisionMismatch` and update error messages that describe nodes as "committed" when they now refer to the live UI DOM.

### Dual-runtime wiring from Burokku

Remove from Burokku's application path:

- `DualRuntimeBuilder`;
- `DualRuntime`;
- `DualRuntimeDriver`;
- `shutdown_with_driver`;
- `main_runtime_plugin`;
- `runtime.background().eval(...)`;
- the current `LocalSet::run_until` and `tokio::select!` composition.

The generic dual-runtime implementation in `crates/runtime` may remain temporarily for other consumers. Burokku should stop using and prominently re-exporting it.

### Obsolete tests, examples, and documentation

Remove publication helper construction from tests in:

- `src/app.rs`;
- `src/ui/host.rs`;
- `src/ui/window_host.rs`;
- `src/ui/layout/engine.rs`;
- `src/ui/layout/tree.rs`;
- `src/ui/scene.rs`;
- `src/ui/text/collect.rs`.

Adapt the behavior tests to pass `&Dom` directly rather than deleting layout, text, scene, or window coverage.

Once `Burokku::run` is synchronous, remove `#[tokio::main]` and the final `.await` from `example/layouts/src/main.rs`. Remove the example's direct Tokio dependency if it is no longer used.

Replace the obsolete two-thread snapshot architecture in `crates/burokku/plan.md` with this document or a short reference to it.

## 2. Code to add

### External-loop application assembly

Add application assembly using:

- `EventLoop::external_runtime_builder`;
- `Builder::external_tick_budget`;
- one persistent `LocalSet`;
- `EventLoop::run_app_external`.

Build the main JavaScript runtime inside a `LocalSet` task, spawn its `RuntimeDriver` with `spawn_local`, evaluate the application script on that runtime, and report initialization failures to the host.

### Main-runtime DOM installation

Change `DomPlugin` so it:

- requires `RuntimeRole::Main` rather than rejecting it;
- returns the shared live DOM handle needed by `ApplicationHost`;
- no longer owns a publisher;
- retains `Plugin::checkpoint` for detached-node reclamation and revision/dirty bookkeeping.

The DOM-facing application script must be evaluated on the main runtime. A background runtime cannot expose the synchronous DOM facade without changing the JavaScript API into asynchronous RPC.

### Startup and shutdown state

Add a small lifecycle controller shared by the LocalSet bootstrap task and `ApplicationHost`:

```rust
enum RuntimeStatus {
    Starting,
    Running,
    Failed(String),
    Stopped,
}
```

The bootstrap task should:

1. build the driven QuickJS runtime;
2. spawn its driver locally;
3. evaluate the application entry script;
4. report startup or unexpected-driver failures;
5. wake the native loop after changing fatal status.

`ApplicationHost::about_to_wait` should inspect this state and request `ActiveEventLoop::exit` on fatal failure.

Shutdown must occur while the external Tokio runtime and LocalSet still exist. The implementation should not attempt to await QuickJS shutdown after `run_app_external` has already dropped them.

### Controlled DOM access operations

Split host work into explicit phases:

- `inspect_dom`: borrow DOM briefly and produce an owned `WindowSpec`;
- `sync_window`: release the DOM borrow, then perform native window operations;
- `build_frame`: borrow DOM immutably for layout and scene materialization;
- `present_frame`: release the DOM borrow, then call WGPU/Vello presentation APIs.

A representative frame helper is:

```rust
fn build_frame_candidate(
    dom: &Dom,
    layout: &mut LayoutEngine<TextEngine>,
    viewport: LogicalViewport,
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
    resources: &mut Resources,
) -> Result<BuiltScene, HostError>;
```

### Direct-DOM layout and scene APIs

Use APIs equivalent to:

```rust
LayoutEngine::compute(
    &mut self,
    dom: &Dom,
    viewport: LogicalViewport,
) -> Result<&ComputedLayout, LayoutError>;

ScenePlan::from_layout(
    dom: &Dom,
    computed: &ComputedLayout,
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> Result<ScenePlan, SceneError>;

BuiltScene::build(
    dom: &Dom,
    computed: &ComputedLayout,
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
    resources: &mut Resources,
) -> Result<BuiltScene, SceneError>;
```

`ComputedLayout` remains an owned cache keyed by DOM revision, viewport, and text-engine generation.

### Optional background worker API

If two JavaScript isolates remain a product requirement, make the distinction explicit:

- the main/UI script owns synchronous DOM APIs;
- an optional worker script runs in the background isolate;
- communication uses bounded channels and owned Rust messages;
- background code never receives DOM references or raw QuickJS values from the main isolate;
- queue overflow and shutdown behavior are explicit.

Do not preserve the current ambiguous single `script` API while silently changing which isolate executes it. Prefer a single UI script by default; if workers are retained, use explicit names such as `ui_script` and `worker_script`.

## 3. Simplification opportunities

### Application builder

Replace:

```rust
runtime: DualRuntimeBuilder
```

with:

```rust
runtime: RuntimeBuilder
```

`runtime_plugin` then installs into the one UI runtime. Remove `main_runtime_plugin`. Console, JSON, and timers are installed once rather than in both isolates.

The crate-level documentation and package description should no longer describe Burokku as a dual-runtime UI crate unless the background worker becomes an explicit optional feature.

### DOM update path

Current path:

```text
JavaScript mutation
  -> lock staging DOM
  -> runtime checkpoint
  -> clone snapshot
  -> ArcSwap store
  -> commit notifier
  -> event-loop wake
  -> reader load
  -> publication reconciliation
```

Target path:

```text
JavaScript mutation
  -> mutate live UI DOM
  -> runtime checkpoint completes the batch
  -> about_to_wait compares revisions
  -> reconcile and request redraw
```

A separate `CommitNotifier` wake is unnecessary because runnable Tokio/QuickJS work already wakes the patched external event loop, and `about_to_wait` follows every bounded external tick.

### Host state machine

Use one revision-driven host state machine instead of a publication reader plus retained latest publication:

```text
DOM revision changed
  -> extract owned WindowSpec
  -> apply native window change
  -> request redraw if a renderer is available

Redraw requested
  -> compute layout from current DOM revision
  -> build owned scene and hit-test plan
  -> release DOM
  -> present
```

Preserve the current recoverable/fatal frame failure policy. A failed candidate frame may leave the previous presented frame installed because that frame already owns its `ScenePlan`.

### Tests

Tests should create `Dom` directly and pass `&Dom` to window, text, layout, and scene APIs. This removes repeated `DomPublisher::new(...).reader.load()` fixtures without reducing behavioral coverage.

## Invariants that must remain

### Batch consistency

Layout and rendering may only observe DOM state after one JavaScript macrotask and all currently ready microtasks have finished. No native callback may synchronously poll QuickJS and then render before the checkpoint completes.

### No long-lived DOM references

Retain only `NodeId`, revisions, computed geometry, shaped text, paint values, and other owned data. Never retain `&Dom`, `&Node`, or an interior reference across callbacks.

### No borrow across reentrant or asynchronous work

Release the live DOM borrow before:

- `.await`;
- native window creation or updates;
- WGPU adapter/device initialization;
- surface creation or replacement;
- Vello/WGPU presentation;
- any operation that may enter a nested platform loop.

### Async graphics authority

Graphics initialization may finish after the desired window has been removed or replaced. Preserve the existing validation using current DOM revision, desired `NodeId`, native `WindowId`, and surface generation. Replace the publication reload with a short live-DOM query; do not delete the authority gate.

### Presented-frame ownership

Pointer hit testing must use the last successfully presented `ScenePlan`, not current live DOM geometry. Validate its `NodeId` against the live DOM immediately before JavaScript event dispatch, and drop stale targets.

### Revision tracking

Keep:

- `Dom::revision()`;
- per-node structure/style/content revisions;
- computed-layout revision;
- scene-plan revision;
- renderer last-presented revision;
- surface generation.

Snapshots are removable; temporal identity is not.

## Migration sequence

### Phase 1: Integrate the external event loop

1. Make `Burokku::run` synchronous.
2. Build patched Tokio through `EventLoop::external_runtime_builder`.
3. Supply one persistent `LocalSet` to `run_app_external`.
4. Drive the existing main QuickJS runtime on that LocalSet.
5. Keep publications temporarily to isolate runner/lifecycle changes.

Exit criteria:

- Burokku compiles against the current winit API;
- QuickJS timers and tasks progress while the window loop runs;
- startup and shutdown failures are surfaced;
- main-runtime tasks remain on the process main thread.

### Phase 2: Move DOM-facing JavaScript to the main runtime

1. Permit and require main-role `DomPlugin` installation.
2. Install the plugin with `main_plugin` while dual-runtime compatibility remains.
3. Evaluate the UI application script on `runtime.main()`.
4. Verify DOM mutation, layout, and scene construction thread identity.
5. Decide whether a background worker is still required.

Exit criteria:

- the synchronous DOM facade runs on the UI thread;
- background JavaScript, if retained, has no direct DOM access;
- framework updates still complete within one runtime checkpoint.

### Phase 3: Replace publications with direct DOM borrowing

1. Give `ApplicationHost` the live DOM state handle.
2. Convert window, layout, text, and scene APIs from publication types to `&Dom`.
3. Build layout and scene under one immutable DOM borrow.
4. Release the borrow before native/GPU work.
5. Replace publication authority checks with live revision and window-identity checks.

Exit criteria:

- no production consumer loads `PublishedDom`;
- a frame's layout, paint data, and hit-test data use one DOM revision;
- retained frames contain no DOM references.

### Phase 4: Delete snapshot machinery

1. Delete `publication.rs`.
2. Remove `arc-swap`.
3. Remove `Dom: Clone` and `Arc<Node>` copy-on-write storage.
4. Delete publication-specific tests and error variants.
5. Convert remaining fixtures to direct DOM usage.

Exit criteria:

- no snapshot/publication symbols remain in `crates/burokku`;
- DOM mutation tests, layout tests, scene tests, and host tests pass;
- stable `NodeId` behavior is unchanged.

### Phase 5: Simplify the public runtime API

1. Change `BurokkuBuilder` to a single `RuntimeBuilder`.
2. Remove `main_runtime_plugin`.
3. Stop re-exporting `DualRuntime` and `DualRuntimeBuilder` as Burokku's primary API.
4. Remove duplicate default plugins.
5. Update the example to a synchronous main function.
6. Optionally add an explicit background worker API.

Exit criteria:

- the default Burokku application creates no dedicated JavaScript background thread;
- one script and one plugin surface have unambiguous UI-runtime ownership;
- optional worker behavior, if present, is explicit.

### Phase 6: Optional local-plugin cleanup

1. Add a non-`Send` `LocalPlugin` path to `crates/runtime`.
2. Restrict it to explicitly driven/local runtimes.
3. Convert `SharedUiDom` from `Arc<Mutex<_>>` to `Rc<RefCell<_>>`.
4. Remove mutex poisoning errors from DOM bindings.

This phase is optional and should follow the functional migration rather than expanding its initial scope.

## Verification plan

### Focused unit tests

- `Dom` stable handles survive moves and mutation without snapshot clones.
- Invalid DOM operations leave revisions and relationships unchanged.
- `collect_paragraph` and `reconcile_full` work directly from `&Dom`.
- Layout cache matching uses DOM revision, viewport, and text generation.
- `ScenePlan` contains owned paint and hit-test data.
- Stale async graphics results are rejected after window replacement or removal.

### Runtime integration tests

- Main QuickJS driver progresses under `run_app_external`.
- DOM plugin installation fails outside the main runtime role.
- A macrotask plus its ready microtasks produces one observable DOM batch.
- Several completed batches may coalesce before one redraw.
- A timer can mount or remove a window after startup.
- Runtime startup failure exits the native host with a useful error.

### Thread-affinity tests

Record and compare thread IDs for:

- event-loop creation;
- DOM binding invocation;
- plugin checkpoint;
- layout computation;
- scene construction;
- presentation callback.

All must be the process main thread.

### Manual macOS validation

1. Start the layouts example.
2. Confirm JavaScript timers continue firing.
3. Resize continuously for at least five seconds.
4. Confirm DOM updates, layout, and redraw continue during the nested live-resize loop.
5. Confirm no snapshot publication code runs and no borrow/reentrancy panic occurs.
6. Close or remove the final window and confirm orderly runtime teardown.

## Risks

### Script-placement compatibility

Applications may currently assume their script runs on the named background thread. Moving the script is an intentional API change and should be documented. Proxying the synchronous DOM facade back to the background isolate would be a larger semantic change and is not recommended.

### Reentrancy

Same-thread state can still be reentered through nested native loops. Keeping DOM borrows short and releasing them before native/GPU operations is mandatory. Tests should exercise live resize and window replacement paths.

### Long synchronous work

Patched Tokio's external tick budget limits task polls, not the duration of one poll. Long JavaScript, layout, shaping, or scene construction still blocks native events. This migration removes transport overhead; it does not provide preemption.

### Shutdown ordering

The native loop owns Tokio and the LocalSet. QuickJS and any worker runtime must stop before that execution environment is destroyed. A background worker, if retained, needs an explicit shutdown-and-join protocol before calling `ActiveEventLoop::exit`.

### Retained failure state

The current host can retain a previously presented frame after a recoverable candidate failure. Ensure all retained frame data is owned before removing the snapshot that previously kept source DOM data alive.

## Final architectural rule

The UI DOM has one owner and one execution thread. JavaScript mutation, checkpointing, native-window reconciliation, layout, scene materialization, and presentation are serialized by the native event loop and patched Tokio scheduler. Background workers communicate through owned messages only. A frame may borrow the live DOM while producing owned derived data, but no DOM borrow or reference survives that frame-building scope.
