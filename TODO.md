# TODO

## Optimize DOM snapshot publication

### Problem

A dirty runtime checkpoint currently deep-clones the entire staging DOM:

```rust
let snapshot = Arc::new(DomSnapshot {
    revision,
    dom: self.staging.clone(),
});
```

This is in `crates/burokku/src/ui/elements/publication.rs`, inside `BtsDom::checkpoint()`. `Dom` currently owns a `SlotMap<NodeId, Node>`, so cloning it copies every live node and recursively clones each node's `Elements`, child `Vec`, attribute/style `BTreeMap`, and owned strings. The work is proportional to the entire arena, even if one attribute changed.

This is more likely to become a bottleneck than the uncontended mutex in `js_bridge.rs`.

### Current architecture and required semantics

Read these files before changing the design:

- `crates/burokku/src/ui/elements.rs`: `Dom`, `Node`, `NodeId`, mutations, and revision tracking.
- `crates/burokku/src/ui/elements/publication.rs`: `BtsDom`, `StagingDomMut`, `DomSnapshot`, `SharedDom`, dirty tracking, and publication.
- `crates/burokku/src/ui/js_bridge.rs`: JavaScript DOM operations against BTS staging state.
- `crates/runtime/src/event_loop.rs`: drains QuickJS microtasks and then invokes plugin checkpoints.
- `crates/burokku/src/ui/computed.rs`: intended MTS-side computed state.

Current data flow:

1. JavaScript invokes native functions from `js_bridge.rs`.
2. Those functions mutate the BTS-owned staging `Dom` through `BtsDom::mutate()`.
3. `StagingDomMut::drop()` marks the batch dirty only if the DOM revision changed.
4. The runtime drains all currently ready QuickJS microtasks.
5. `DomPlugin::checkpoint()` calls `BtsDom::checkpoint()` once after that macrotask and its microtasks.
6. A complete immutable `DomSnapshot` is published through `ArcSwap`.
7. MTS is expected to call `SharedDom::load()` once per frame and retain that exact `Arc<DomSnapshot>` for the frame.
8. A Tokio watch channel supplies coalescing revision notifications.

Any optimization must preserve these properties unless the replacement architecture explicitly documents a changed contract:

- A loaded snapshot never changes while a reader holds its `Arc`.
- Readers see either the complete old revision or the complete new revision, never a partially mutated tree.
- Several mutations in one macrotask/microtask checkpoint publish one complete revision.
- Successful mutations before a JavaScript exception are still published.
- Invalid and no-op operations do not dirty or publish a new revision.
- `NodeId` remains stable across moves, updates, and snapshots.
- Removed IDs remain stale even if their slot is later reused; generation checking must not regress.
- Detached JavaScript nodes remain valid and can be reinserted.
- Snapshot publication must remain thread-safe and must not block MTS for the duration of a BTS mutation batch.
- Do not introduce `unsafe`; the binary crate currently has `#![forbid(unsafe_code)]`.

### Recommended implementation path

Implement this in stages and benchmark after every stage. Do not jump directly to a custom arena.

#### Stage 0: establish benchmarks

Add non-flaky release-mode benchmarks; do not add timing assertions to unit tests. Measure at least:

- Checkpoint of clean DOM: should remain effectively constant and allocate nothing.
- Dirty checkpoint for approximately 100, 1,000, and 10,000 nodes.
- One changed attribute in a large DOM.
- A structural change affecting a parent and child in a large DOM.
- A render-like batch that creates and attaches many nodes.
- MTS traversal/read cost for a complete reachable tree.
- Peak retained memory while MTS holds the previous snapshot and BTS publishes the next one.

Record both checkpoint latency and, if practical, allocation count/bytes. Compare the current implementation against each later stage. The important cases are a large DOM with a small changed set and a large DOM with most nodes changed.

#### Stage 1: shallow snapshots with `Arc<Node>`

This is the lowest-risk first optimization.

Change the arena from:

```rust
nodes: SlotMap<NodeId, Node>
```

to approximately:

```rust
nodes: SlotMap<NodeId, Arc<Node>>
```

Create entries with `Arc::new(Node { ... })`. Read APIs can continue returning `&Node` by dereferencing the `Arc`. Add one internal helper for mutations:

```rust
fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, DomError> {
    let node = self.nodes.get_mut(id).ok_or(DomError::NodeNotFound(id))?;
    Ok(Arc::make_mut(node))
}
```

Update mutation methods to validate the full operation before calling `Arc::make_mut`. Structural mutations often affect an old parent, a new parent, and the child; mutate each entry only after validation so errors still leave the tree unchanged.

Special case to handle: `remove_subtree()` currently owns the removed `Node` and returns its `Elements`. With `Arc<Node>`, an old snapshot may still share the node. Do not rely on `Arc::try_unwrap`; clone the returned `Elements` when necessary.

Expected result:

- Checkpoint remains O(total node count), because cloning `SlotMap` still copies every slot.
- Unchanged node contents, child vectors, maps, and strings are shared instead of deeply cloned.
- The first mutation of a node after publication copies only that node through `Arc::make_mut`.
- Existing direct `SlotMap` lookup performance is retained.

Stop here if benchmarked checkpoint and memory costs are already acceptable. This stage is simpler and safer than introducing persistent collections.

Add tests proving that:

- A published old snapshot is unchanged after staging mutates a node shared with it.
- Mutating one node does not clone or alter unrelated nodes. `Arc::ptr_eq` may be exposed only to module tests through a private helper.
- Structural changes preserve old and new parent/child relationships in their respective snapshots.
- Removing a shared subtree does not invalidate it in an old snapshot.

#### Stage 2: persistent snapshot map for O(1) publication

Only do this if Stage 1 benchmarks show that copying all `SlotMap` entries remains significant.

The central issue is that `SlotMap` currently serves two responsibilities:

1. BTS-only allocation and generation tracking.
2. Node storage included in every snapshot.

Split those responsibilities. Keep a mutable `SlotMap<NodeId, ()>` or equivalent allocator exclusively in `BtsDom`, and store snapshot-visible nodes in a structurally shared persistent map, for example:

```rust
struct MutableDom {
    ids: SlotMap<NodeId, ()>,
    view: Dom,
}

#[derive(Clone)]
pub struct Dom {
    root: NodeId,
    revision: u64,
    nodes: im::HashMap<NodeId, Arc<Node>>,
}
```

`Dom` then becomes the immutable/readable view that can be cloned cheaply. The ID allocator is never placed in `DomSnapshot`, so it is never cloned for publication.

Allocation flow:

1. Insert `()` into the BTS-only `SlotMap` to obtain a generation-checked `NodeId`.
2. Insert `Arc<Node>` under that ID in the persistent map.
3. On removal, remove the node from the persistent map and release the ID from the BTS allocator.
4. Old snapshots retain old map entries. If the slot index is reused, SlotMap's new generation produces a different `NodeId`, preserving stale-handle behavior.

Mutation flow:

1. Validate using the current persistent view.
2. Clone the selected `Arc<Node>`.
3. Use `Arc::make_mut` to modify that node.
4. Insert the changed `Arc<Node>` back into the persistent map.
5. Persistent-map updates copy only the trie/path needed for changed keys.

Checkpoint becomes approximately:

```rust
let snapshot = Arc::new(DomSnapshot {
    revision,
    dom: self.staging.view.clone(), // persistent root clone
});
```

Expected complexity:

- Snapshot publication: O(1), excluding `ArcSwap` and notification work.
- Node update: proportional to the persistent-map path plus the changed node's local data.
- Old snapshots share all unchanged map branches and node values.
- Old map branches are freed automatically when no snapshots reference them; do not build an explicit parent-snapshot chain that retains unbounded history.

Likely refactor: `StagingDomMut` currently dereferences directly to `Dom`, and `Dom::create()` owns ID allocation. Once allocation is split, `StagingDomMut` should hold both the mutable allocator and persistent view and provide forwarding mutation methods. It may continue to dereference read-only to `Dom`, but creation/removal should be explicit methods on `StagingDomMut` or another mutable facade.

Before selecting `im::HashMap`, benchmark MTS tree traversal. Persistent hash lookup has more pointer chasing than `SlotMap` indexing. If read regression is too large, consider a persistent vector or paged COW arena, but only with benchmark evidence.

Required Stage 2 tests:

- Keep all current `elements.rs`, `publication.rs`, and `js_bridge.rs` tests passing.
- Assert `Dom`, `DomSnapshot`, and `SharedDom` remain `Send + Sync`.
- Hold several historical snapshots while repeatedly updating and removing nodes; verify every revision independently.
- Remove a node, reuse its allocator slot, and verify old/new generations resolve only in their corresponding snapshots.
- Verify detached nodes are represented consistently and can later be attached.
- Verify dropping old snapshots releases old nodes; use `Weak<Node>` in module tests if helpful.
- Stress concurrent MTS loads while BTS publishes revisions and validate complete-tree invariants.

#### Stage 3: publication cadence/coalescing

Current publication occurs once after every dirty macrotask and its microtasks. `ArcSwap` and the watch channel can coalesce what MTS observes, but BTS may still construct snapshots that are overwritten before any frame consumes them.

Once the MTS frame loop exists, measure whether multiple dirty checkpoints commonly occur between frames. If so, consider publishing at most once per frame or in response to an MTS snapshot request.

This changes timing semantics: current tests can await a commit immediately after `Runtime::eval()`. Do not silently defer publication without defining how evaluations, redraw notifications, shutdown, and JavaScript exceptions flush pending changes. A pending dirty DOM must be flushed before shutdown and wherever callers require read-after-eval visibility.

Persistent publication from Stage 2 may make intermediate snapshots cheap enough that cadence changes are unnecessary.

### Alternative: send DOM deltas to an MTS-owned copy

If preserving `SharedDom::load() -> Arc<DomSnapshot>` is not required, a delta architecture may produce the best rendering performance:

```rust
struct DomDelta {
    revision: u64,
    changed: Vec<(NodeId, Arc<Node>)>,
    removed: Vec<NodeId>,
}
```

BTS records changed node IDs during a batch and sends one delta after the checkpoint. MTS owns a local mutable `SlotMap`-like DOM, drains deltas only between frames, and never mutates it during a frame.

Advantages:

- BTS publication and MTS application are O(changed nodes).
- MTS retains fast indexed node reads.
- No full immutable DOM clone is needed.

Costs and hazards:

- Deltas cannot be dropped merely because a watch channel coalesced notifications; later deltas depend on earlier ones. Use an ordered queue or make each message independently reconstructible.
- Backpressure and resynchronization need explicit designs.
- MTS no longer obtains arbitrary immutable historical snapshots through `SharedDom::load()`.
- Strings/node data may need `Arc` ownership to avoid copying into both BTS and MTS.
- Frame-boundary application becomes part of correctness.

Do not combine a delta migration with the persistent-map migration in one change. Choose based on whether immutable snapshot loading remains an architectural requirement.

### Last-resort option: paged copy-on-write arena

If persistent-map reads are too slow and deltas do not fit the API, implement a custom generational arena divided into shared fixed-size pages, such as `Vec<Arc<Page>>`. Snapshot creation clones only page pointers; mutation clones affected pages. Pair this with per-node `Arc` values if pages otherwise deep-clone node contents.

This can approach indexed `SlotMap` read performance, but allocator generations, free lists, stale handles, and page reclamation become project-owned correctness risks. Do not choose this before Stage 1/2 benchmarks demonstrate a need.

### Completion criteria

The optimization is complete when:

- Release benchmarks show that a one-node update no longer copies data proportional to the entire DOM, or the remaining O(N) shallow copy is documented as acceptable.
- Snapshot readers retain immutable, complete revisions without taking the BTS mutex.
- Existing JavaScript DOM and publication behavior remains intact.
- Generation-checked IDs and detached-node behavior are preserved.
- No-op and invalid mutations still avoid publication.
- Memory does not grow with the number of historical revisions once old snapshot `Arc`s are dropped.
- The selected design and benchmark results are documented near `BtsDom::checkpoint()`.
