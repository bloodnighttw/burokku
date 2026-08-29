# DOM and Layout Colocation Plan

## Status

Draft migration plan for replacing Burokku's cross-thread DOM publication model with a UI-thread-owned live DOM driven by the patched Tokio/native event-loop integration.

## Summary

Layout already runs on the UI thread inside `ApplicationHost`; the mutable DOM and application JavaScript currently run in the background runtime. The patched Tokio integration allows the main QuickJS driver to progress inside the native event loop, so the DOM-facing JavaScript runtime can move onto the UI thread beside window management, layout, scene construction, and presentation.

The final architecture uses one JavaScript runtime for the Burokku application. `DualRuntime`, its dedicated background JavaScript thread, and its cross-runtime bridge are removed rather than retained as optional infrastructure.

Application code that needs asynchronous I/O continues to use Tokio. CPU-intensive or blocking Rust work must use `spawn_blocking` or an explicit non-JavaScript worker designed separately from the UI runtime.

```text
AppKit event loop
  -> patched Tokio bounded tick
  -> main QuickJS macrotask and ready microtasks
  -> bounded Tokio tick returns
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
5. Remove `DualRuntime` and make Burokku's JavaScript runtime explicitly local to the UI thread.
6. Preserve stale-result checks for asynchronous GPU work and generation-checked node handles.

## Non-goals

- Removing DOM revisions or `NodeId` generations.
- Reintroducing a background JavaScript isolate or cross-runtime DOM bridge.
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

Discarding `DualRuntime` allows the runtime and plugin APIs to become explicitly thread-local. `Plugin` no longer needs a global `Send` bound, and the QuickJS driver must be `!Send` and driven only through the persistent `LocalSet`. The live DOM state can therefore use `Rc<RefCell<_>>`:

```rust
struct UiDomState {
    dom: Dom,
    live_wrappers: HashMap<NodeId, usize>,
    last_reclaim: ReclaimReport,
}

type SharedUiDom = Rc<RefCell<UiDomState>>;
```

`DomPlugin` and `ApplicationHost` each hold an `Rc` clone pointing to the same `UiDomState`; the DOM itself is not cloned. Access should use `try_borrow` and `try_borrow_mut` so accidental reentrancy becomes an explicit host or JavaScript error rather than a panic.

No `RefCell` borrow may be held across `.await`, native window operations, presentation, JavaScript execution, or any operation that may enter a nested platform loop.

### Frame transaction

A frame must materialize all DOM-dependent data under one uninterrupted immutable DOM borrow:

1. read `dom.revision()`;
2. compute or reuse layout for that revision, viewport, and text generation;
3. build a fully owned Vello scene and `ScenePlan` from the same DOM revision;
4. assert that the DOM revision has not changed;
5. release the DOM borrow;
6. present the built scene.

The retained `ComputedLayout`, `BuiltScene`, `ScenePlan`, and `PresentedFrame` must not contain Rust references into `Dom`.

### `about_to_wait` scheduling

The target architecture removes `Plugin::checkpoint` and the runtime's post-macrotask plugin iteration. The production checkpoint override is used only by `DomPlugin`; its snapshot-publication responsibility disappears, and its detached-node reclamation moves to `ApplicationHost::about_to_wait`.

The local QuickJS task executes one synchronous macrotask and drains its ready microtasks before yielding back to Tokio. `run_app_external` invokes `about_to_wait` after the bounded Tokio tick returns, so the host cannot observe the middle of that JavaScript work. One tick may complete several JavaScript tasks; processing only the latest DOM revision intentionally coalesces them into one native reconciliation and redraw request.

`about_to_wait` should:

1. acquire `UiDomState` with `try_borrow_mut`;
2. reclaim detached nodes;
3. read the current DOM revision and extract any owned state needed for reconciliation;
4. release the `RefCell` borrow;
5. clear caches for reclaimed nodes;
6. reconcile the native window when the revision changed;
7. request a redraw when a renderer is available.

Actual layout, scene construction, and presentation remain in `WindowEvent::RedrawRequested`. Native callbacks that dispatch JavaScript events must enqueue a macrotask and must not synchronously poll JavaScript from inside the callback.

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

### Plugin checkpoint API

Remove from `crates/runtime`:

- `Plugin::checkpoint`;
- post-macrotask plugin checkpoint iteration in `src/event_loop.rs`;
- checkpoint-specific error logging and documentation;
- checkpoint-only tests such as `RecordingCheckpoint` and `plugin_checkpoint_runs_after_microtasks_and_failed_macrotasks`.

Remove `DomPlugin::checkpoint`. Move detached-node reclamation and cache cleanup to the host's `about_to_wait` processing. Keep `Plugin` as an installation-only interface.

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

### Dual-runtime implementation

Remove from `crates/runtime`:

- `src/dual_runtime.rs`;
- `DualRuntime`, `DualRuntimeBuilder`, and `DualRuntimeDriver` exports;
- dedicated background-thread creation, shutdown, and join logic;
- main/background plugin collections and queue-capacity configuration;
- dual-runtime-specific tests;
- `src/bridge.rs` and its exports/tests;
- the `RuntimeRole` mechanism and main/background role branches.

Remove from Burokku:

- `DualRuntimeBuilder`, `DualRuntime`, and `DualRuntimeDriver` imports and exports;
- `shutdown_with_driver`;
- `main_runtime_plugin`;
- `runtime.background().eval(...)`;
- duplicate Console, JSON, and Timers plugin installation;
- the current `LocalSet::run_until` and `tokio::select!` composition.

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

- installs only in the one local QuickJS runtime;
- returns the shared live DOM handle needed by `ApplicationHost`;
- no longer owns a publisher;
- no longer implements a post-task checkpoint; detached-node reclamation is owned by `ApplicationHost::about_to_wait`.

The application script is evaluated on this same local runtime. Runtime roles and main/background installation branches are unnecessary once `DualRuntime` is removed.

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

### Explicitly local runtime API

With `DualRuntime` removed, `crates/runtime` should make thread affinity part of its type contract:

- change `Plugin: Send + 'static` to `Plugin: 'static`;
- store local plugin trait objects without a `Send` bound;
- make `RuntimeDriver` explicitly `!Send`, for example with `PhantomData<Rc<()>>`;
- drive it only with `tokio::task::spawn_local`;
- remove or redesign `Runtime::build` if it automatically uses `tokio::spawn`;
- prefer `RuntimeBuilder::build_driven` under the event loop's persistent `LocalSet`;
- remove rquickjs's `parallel` feature so QuickJS's type-level contract matches local execution;
- update runtime tests to enter a current-thread runtime and persistent `LocalSet`.

Asynchronous networking, timers, and synchronization continue to use Tokio. Blocking or CPU-intensive Rust work must be explicitly offloaded with `spawn_blocking`; no background JavaScript runtime is retained.

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

The crate-level documentation and package description should describe one thread-affine UI JavaScript runtime rather than a dual-runtime UI crate.

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
  -> QuickJS task drains ready microtasks and yields
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

Layout and rendering may only observe DOM state after one JavaScript macrotask and all currently ready microtasks have finished and the bounded Tokio tick has returned. No native callback may synchronously poll QuickJS and then render before that scheduler boundary.

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

### Phase 1: Make `crates/runtime` local and remove `DualRuntime`

1. Delete `dual_runtime.rs` and its exports/tests.
2. Delete `bridge.rs` and its exports/tests.
3. Remove main/background runtime roles and builder branches.
4. Change `Plugin: Send + 'static` to `Plugin: 'static`.
5. Make `RuntimeDriver` explicitly `!Send`.
6. Replace automatic `tokio::spawn` driving with explicit `build_driven` plus `spawn_local`.
7. Remove the rquickjs `parallel` feature.
8. Remove `Plugin::checkpoint`, runtime checkpoint iteration, and checkpoint-only tests.
9. Update remaining runtime tests to use a current-thread runtime and `LocalSet`.

Exit criteria:

- no `DualRuntime`, background JavaScript thread, or runtime bridge symbols remain;
- the compiler prevents moving `RuntimeDriver` to another thread;
- QuickJS evaluation, timers, promises, plugin installation, and Tokio I/O still pass on a `LocalSet`;
- no runtime path calls `tokio::spawn(driver.run())`.

### Phase 2: Integrate the external event loop and single runtime

1. Change `BurokkuBuilder` from `DualRuntimeBuilder` to `RuntimeBuilder`.
2. Make `Burokku::run` synchronous.
3. Build patched Tokio through `EventLoop::external_runtime_builder`.
4. Supply one persistent `LocalSet` to `run_app_external`.
5. Build and drive the single QuickJS runtime on that LocalSet.
6. Install Console, JSON, Timers, and `DomPlugin` once.
7. Evaluate the application script on the single runtime.
8. Add startup, fatal-error, and shutdown lifecycle reporting.
9. Keep publications temporarily to isolate event-loop and runtime-lifecycle changes.

Exit criteria:

- Burokku compiles against the current winit API;
- QuickJS timers and tasks progress while the native loop runs;
- application JavaScript, DOM bindings, `about_to_wait` housekeeping, layout, and scene construction run on the process main thread;
- startup and shutdown failures are surfaced;
- Burokku creates no dedicated JavaScript background thread.

### Phase 3: Replace publications with direct local DOM borrowing

1. Change shared DOM state to `Rc<RefCell<UiDomState>>`.
2. Give `DomPlugin` and `ApplicationHost` clones of the same local handle.
3. Use `try_borrow` and `try_borrow_mut` to report reentrancy.
4. Convert window, layout, text, and scene APIs from publication types to `&Dom`.
5. Build layout and scene under one immutable DOM borrow.
6. Release the borrow before native or GPU work.
7. Replace publication authority checks with short live-DOM revision and window-identity checks.
8. Move detached-node reclamation and reclaimed-cache cleanup to `ApplicationHost::about_to_wait`.

Exit criteria:

- no production consumer loads `PublishedDom`;
- a frame's layout, paint data, and hit-test data use one DOM revision;
- retained frames contain no DOM references;
- no `RefCell` borrow survives an `.await`, native operation, presentation, or JavaScript invocation.

### Phase 4: Delete snapshot and copy-on-write machinery

1. Delete `publication.rs`.
2. Remove `arc-swap`.
3. Remove `Dom: Clone` and `Arc<Node>` copy-on-write storage.
4. Delete publication-specific tests and error variants.
5. Convert remaining fixtures to direct DOM usage.
6. Remove publication notification and reader code from `ApplicationHost`.

Exit criteria:

- no snapshot/publication symbols remain in `crates/burokku`;
- DOM mutation, layout, scene, and host tests pass;
- stable `NodeId` behavior is unchanged;
- frame and presentation revisions continue to detect stale work.

### Phase 5: Simplify public APIs, examples, and documentation

1. Remove `main_runtime_plugin`; `runtime_plugin` targets the one local runtime.
2. Stop exporting `DualRuntime` and `DualRuntimeBuilder`.
3. Remove duplicate default plugins and dual-runtime shutdown helpers.
4. Update the example to a synchronous main function.
5. Remove the example's direct Tokio dependency if unused.
6. Update crate descriptions and architecture documents to describe one local UI runtime.
7. Remove dual-runtime and snapshot benchmarks; retain layout, scene, and responsiveness measurements.

Exit criteria:

- one script and one plugin surface have unambiguous UI-runtime ownership;
- documentation contains no optional background JavaScript runtime path;
- the layouts example starts, updates by timer, renders, and shuts down correctly.

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
- DOM plugin and application script execute through the one local QuickJS runtime.
- A macrotask plus its ready microtasks produces one observable DOM batch.
- Several completed batches may coalesce before one redraw.
- `about_to_wait` reclaims detached nodes before layout and clears their derived caches.
- Successful DOM mutations made before a JavaScript exception are observed on the next `about_to_wait` pass.
- The runtime plugin API is installation-only and has no post-task checkpoint callback.
- A timer can mount or remove a window after startup.
- Runtime startup failure exits the native host with a useful error.

### Thread-affinity tests

Record and compare thread IDs for:

- event-loop creation;
- DOM binding invocation;
- `about_to_wait` detached-node reclamation and revision observation;
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

Applications currently run their script on the named background thread. Removing `DualRuntime` intentionally moves that script to the process/UI thread. This is a breaking thread-affinity change and must be documented; no compatibility bridge or background JavaScript mode is retained.

### Reentrancy

Same-thread state can still be reentered through nested native loops. Keeping DOM borrows short and releasing them before native/GPU operations is mandatory. Tests should exercise live resize and window replacement paths.

### Long synchronous work

Patched Tokio's external tick budget limits task polls, not the duration of one poll. Long JavaScript, layout, shaping, or scene construction still blocks native events. This migration removes transport overhead; it does not provide preemption.

### Shutdown ordering

The native loop owns Tokio, the LocalSet, and the one QuickJS runtime. QuickJS shutdown must complete while that execution environment still exists; `run_app_external` must not drop the LocalSet or Tokio runtime before the local driver has been asked to stop.

### Retained failure state

The current host can retain a previously presented frame after a recoverable candidate failure. Ensure all retained frame data is owned before removing the snapshot that previously kept source DOM data alive.

## Final architectural rule

The UI DOM and the single QuickJS runtime have one owner thread. JavaScript mutation is completed by the local Tokio task before `about_to_wait` performs detached-node reclamation, revision observation, native-window reconciliation, and redraw scheduling. Layout, scene materialization, and presentation then run through the native redraw lifecycle. `DualRuntime`, plugin checkpointing, the background JavaScript thread, and the cross-runtime bridge are not part of the target architecture. A frame may borrow the live DOM while producing owned derived data, but no DOM borrow or reference survives that frame-building scope.
