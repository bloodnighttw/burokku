# Burokku UI Runtime Plan

## Scope and assumptions

Burokku uses two JavaScript runtimes, similar to LynxJS:

- **MTS (Main Thread Script):** lives on the process/UI thread with the native window, event loop, layout engine, and Vello renderer.
- **BTS (Background Thread Script):** lives on a dedicated background thread and runs application code such as Solid updates.

The details of what application code should run on MTS are outside the scope of this plan. It is expected and acceptable that native window callbacks, synchronous JavaScript, layout, or drawing temporarily prevent other MTS work from executing.

## Design goals

1. MTS and BTS can refer to the same logical DOM-like tree through stable node handles.
2. MTS never performs layout or drawing from a partially updated tree.
3. Solid can perform multiple DOM operations and publish them as one coherent update.
4. Layout, hit testing, Vello scene construction, and presentation remain on MTS.
5. No shared lock is held during JavaScript execution, layout, drawing, GPU work, or an `.await` point.
6. Multiple commits may be coalesced when MTS has not rendered yet.

## Runtime ownership

### MTS owns

- The native window and event loop
- The main JavaScript isolate
- Taffy layout state
- Computed layout and hit-testing data
- Vello renderer, scenes, surfaces, and presentation
- The currently presented DOM revision

### BTS owns

- The background JavaScript isolate
- The mutable staging DOM used by application code
- The mutation batch currently being assembled
- Publication of committed DOM revisions

### Shared state

MTS and BTS share a small thread-safe handle rather than concurrently mutating the same tree:

```rust
struct SharedDom {
    committed: arc_swap::ArcSwap<DomSnapshot>,
}
```

`DomSnapshot` is immutable after publication and contains a monotonically increasing revision number. BTS publishes a complete `Arc<DomSnapshot>`, while MTS loads one snapshot and retains that same `Arc` for the complete layout-and-render operation.

This provides shared access without making MTS wait for BTS or allowing MTS to observe an intermediate mutation.

## Why not `tokio::Mutex<DomTree>`

A Tokio mutex around the entire mutable tree is not the default design. The raw cost of acquiring a mutex is less important than the duration and location of its critical section:

- Synchronous JavaScript DOM methods cannot naturally wait on an async mutex.
- Per-operation locking allows rendering to observe partially completed Solid updates.
- Holding a lock for an entire Solid update would run arbitrary JavaScript while blocking MTS.
- Holding a lock during Taffy layout or Vello drawing could block BTS for a significant part of a frame.
- Native event callbacks and drawing paths should not block waiting for ownership of the DOM.

If a fallback mutex is ever required, it must protect only a short pointer clone or pointer swap such as `Arc<DomSnapshot>`. Layout and rendering must happen after releasing it. `ArcSwap` is preferred because MTS readers do not need to acquire such a lock.

Performance assumptions must eventually be verified with benchmarks; they should not be guaranteed solely from the synchronization primitive selected.

## DOM representation

The mutable DOM should use stable node identifiers rather than Rust references or a recursively nested enum as its primary storage format.

```rust
struct NodeId {
    index: u32,
    generation: u32,
}

struct Dom {
    nodes: SlotMap<NodeId, Node>,
    root: NodeId,
}
```

The exact arena implementation may change, but it must provide:

- Stable handles for JavaScript wrappers and native events
- Generation checking so deleted handles cannot access reused nodes
- Parent and child relationships
- Efficient insertion, removal, and movement
- Per-node style and content revisions or dirty flags
- Validation when a mutation is applied

Invalid relationships should be rejected or normalized during mutation. They should not remain in the authoritative tree and merely be skipped by the renderer, because that would make JavaScript and rendering disagree about the DOM.

Neither runtime should retain a direct pointer or Rust reference to a node across a commit. It should retain only a `NodeId`.

## Batch update model

### Staging

DOM operations issued by Solid update BTS's mutable staging DOM immediately. This allows JavaScript running within the same update to read its own mutations.

The first mutation in a JavaScript task marks the staging DOM as dirty. Further mutations are added to the same pending batch without publishing intermediate snapshots.

### Commit boundary

The default automatic commit boundary is:

1. Execute one JavaScript macrotask.
2. Drain all currently ready QuickJS microtasks.
3. Validate/finalize the pending DOM mutations.
4. Publish one complete DOM revision.
5. Notify MTS and request a redraw once.

The runtime already has a natural checkpoint after draining QuickJS jobs in `crates/runtime/src/event_loop.rs`. A DOM/plugin checkpoint hook should be added there.

Explicit nested `beginBatch`/`endBatch` support may be added later, but normal Solid rendering should not require it.

### Exceptions and invalid mutations

A JavaScript exception does not automatically roll back mutations that already succeeded. At the checkpoint, successfully applied changes are committed together. Invalid individual operations are rejected when attempted and must leave the staging DOM valid.

### Publication guarantee

MTS sees either the complete old revision or the complete new revision. It never sees the tree between two operations in the same batch.

## Snapshot and mutation transport

The initial implementation may create a complete immutable snapshot at commit time. This is acceptable for correctness and early development, but its cost must be measured with realistically sized trees.

If complete snapshots become too expensive, evolve to one of these approaches without changing batch semantics:

1. A persistent/chunked arena that shares unchanged storage between revisions.
2. A complete mutation batch sent to an MTS-owned render-tree mirror.
3. A hybrid containing an immutable snapshot for shared queries and a mutation batch for incremental MTS updates.

Regardless of representation, publication must remain atomic at the revision level.

If both runtimes eventually need to initiate DOM mutations, they must send complete batches to one designated mutation owner. They must not concurrently mutate the same arena through a shared mutex.

## MTS frame lifecycle

When MTS receives a commit notification or native event:

1. Load the latest committed `Arc<DomSnapshot>`.
2. Keep that exact snapshot for the whole frame.
3. If its revision differs from the computed revision, update or rebuild Taffy state.
4. Calculate layout on MTS.
5. Build hit-testing data and the Vello scene from the same revision.
6. Draw and present the scene.
7. Record the presented revision.

If BTS publishes another revision during these steps, the current frame finishes with its existing snapshot. MTS may process the newer revision on the next frame.

MTS may skip intermediate committed revisions and render only the latest one, provided each rendered revision is internally complete.

No DOM synchronization lock may be held during these steps.

## Events and hit testing

Hit testing must use the computed scene associated with the currently presented or currently processed revision, not an unrelated newer snapshot.

An event target is represented by `NodeId` plus the relevant DOM/presented revision when needed. MTS dispatches the event to the appropriate JavaScript runtime through its bounded macrotask queue. Native callbacks must not synchronously wait for BTS to execute JavaScript.

The Phase 5 event policy is:

- Pointer and wheel events are hit-tested against the last successfully presented layout, using its display scale and DOM revision. Keyboard and focus events target the presented `Window` node until element focus management is implemented.
- Each queued event carries the stable target `NodeId` and presented revision. BTS validates the generation immediately before JavaScript dispatch; a stale or deleted target is dropped rather than retargeted.
- Native callbacks submit with the bounded BTS macrotask queue's non-blocking API. If the queue is full, the newest event is dropped and counted. If the queue is closed, MTS requests application shutdown.

## Scheduling and responsiveness

The MTS runtime driver and asynchronous native event loop are cooperatively polled on the main thread. Synchronous JavaScript, event handling, layout, and scene construction can delay one another; this is expected behavior.

The implementation should still follow these rules:

- Make redraw demand-driven rather than continuously rebuilding unchanged scenes.
- Coalesce repeated commit notifications.
- Do not wait synchronously for BTS from MTS.
- Do not hold shared locks across `.await`.
- Do not hold shared locks during JavaScript, layout, rendering, or GPU submission.
- Use bounded queues and define behavior for queue backpressure.
- Keep revision and frame timing metrics so stalls can be diagnosed.

## Error and shutdown behavior

- A failure to build layout or a Vello scene must not expose a partial computed revision.
- MTS should retain the last valid scene when recovery is possible.
- Closing the final window should request shutdown of both runtimes.
- MTS must continue polling its runtime driver while shutdown is in progress.
- The BTS thread must be joined without blocking the native event callback directly.

## Implementation phases

### Phase 1: DOM foundation

- Replace the recursive tree as authoritative mutable storage with an arena and stable `NodeId` values.
- Add parent/child mutation operations and structural validation.
- Add revision tracking and stale-handle tests.
- Implement the BTS JavaScript bridge in `src/ui/js_bridge.rs`.

### Phase 2: Atomic batches

- Add a pending mutation batch to the BTS DOM owner.
- Add the post-macrotask/post-microtask checkpoint hook to the runtime.
- Publish immutable `DomSnapshot` values through `ArcSwap`.
- Add tests proving that MTS cannot observe intermediate mutations.
- Add tests for exceptions, invalid operations, nested updates, and stale nodes.

### Phase 3: MTS computed state

- Implement MTS-owned computed/layout state in `src/ui/computed.rs`.
- Convert committed DOM revisions into Taffy nodes.
- Add dirty propagation or initially rebuild layout per revision.
- Ensure layout and hit-testing data carry their source revision.

### Phase 4: Window and Vello integration

- Run the native event loop alongside the MTS runtime driver.
- Create and own Vello rendering state on MTS.
- Build scenes only from complete committed revisions.
- Coalesce redraw requests and retain the loaded snapshot for each frame.

### Phase 5: Event dispatch

- Implement hit testing against the presented layout revision.
- Dispatch events using stable `NodeId` handles and bounded runtime queues.
- Define stale-target, deleted-node, and backpressure behavior.

### Phase 6: Performance validation

Benchmark at least:

- Commit latency and snapshot publication latency
- MTS snapshot load latency
- Full-snapshot creation cost by tree size
- Layout time for clean, partially dirty, and fully dirty trees
- Scene construction and Vello rendering time
- Commit-to-present latency
- Dropped/coalesced revisions
- MTS and BTS queue depth under load

Optimize snapshot storage or introduce an MTS mirror only after measurements show that complete snapshots are a meaningful bottleneck.

Phase 6 is implemented by the optimized headless harness in `benches/phase6.rs`
and debug-only `PerformanceMetrics` instrumentation. Run the CPU suite with
`cargo bench -p burokku --bench phase6`; the benchmark profile enables debug
assertions for the diagnostic counters. Run a debug window with
`pnpm --filter @burokku/example-counter build` followed by
`BUROKKU_PRINT_METRICS=1 cargo run -p burokku-example-counter` to capture
Vello render/present and commit-to-present latency, which cannot be represented
faithfully by a headless benchmark. The
full procedure and metric mapping are documented in `benches/README.md`.
Baseline measurements show snapshot creation and publication remain below layout
cost at the tested tree sizes, so this phase does not yet introduce an MTS mirror
or a different snapshot representation.

## Required correctness tests

- MTS observes only old or new revisions, never an intermediate tree.
- Several Solid DOM operations produce one redraw notification.
- A commit during layout is deferred to a later frame.
- Layout, hit testing, and rendering use the same revision.
- Moving and deleting nodes preserve handle-generation correctness.
- Invalid parent/child operations do not corrupt the staging DOM.
- A JavaScript exception commits previously successful mutations coherently.
- A slow BTS update does not hold a lock needed by MTS.
- A slow MTS frame does not prevent BTS from editing its staging DOM.
- Multiple commits before a frame can be coalesced safely.

## Final architectural rule

The shared object is a publication mechanism for immutable committed revisions, not a mutex granting concurrent mutable access to one DOM arena. BTS stages and commits changes; MTS consumes complete revisions for layout and Vello rendering.
