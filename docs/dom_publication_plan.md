# Immutable DOM publication implementation plan

This plan addresses **Problem 2: Immutable DOM publication is absent** from
`docs/dom_foundation_review.md`.

## Goal

Keep one mutable DOM on the background thread (BTS), and publish complete,
immutable revisions for the main thread (MTS):

```text
BTS mutates staging Dom synchronously
    -> runtime finishes one macrotask and ready microtasks
    -> checkpoint detects a changed Dom revision
    -> clone staging Dom into PublishedDom { snapshot, changes }
    -> atomically store one Arc<PublishedDom> in ArcSwap
    -> notify/wake MTS
    -> MTS loads and retains one Arc for the whole frame
```

The first version will always emit a full-rebuild marker. Incremental change
discovery is Problem 3 and is not required here.

## Current foundation to reuse

- `Dom` already stores `SlotMap<NodeId, Arc<Node>>` and derives `Clone`.
- Cloning the slot map preserves every generation-checked `NodeId` and only
  clones each node's `Arc` initially.
- Later staging mutations use `Arc::make_mut`, so nodes shared with a published
  revision are copied before modification.
- `Dom::revision()` advances on effective mutation, while existing no-op
  mutation paths leave it unchanged.
- `runtime::Plugin::checkpoint` already runs after every macrotask and all ready
  QuickJS jobs, including after a failed JavaScript macrotask.
- `arc-swap` is already a dependency of `crates/burokku`.

## Proposed publication model

Implement the following concepts in
`crates/burokku/src/ui/elements/publication.rs`.

```rust
pub enum ChangeSet {
    FullRebuild {
        from_revision: u64,
        to_revision: u64,
    },
}

pub struct DomSnapshot {
    dom: Dom,
}

pub struct PublishedDom {
    snapshot: DomSnapshot,
    changes: ChangeSet,
}

#[derive(Clone)]
pub struct PublishedDomReader {
    committed: Arc<ArcSwap<PublishedDom>>,
}

pub struct DomPublisher {
    committed: Arc<ArcSwap<PublishedDom>>,
    last_published_revision: u64,
    notifier: Arc<dyn CommitNotifier>,
}
```

Names and visibility may be adjusted to match crate conventions, but preserve
these ownership rules:

- `DomPublisher` is single-owner and lives on BTS; do not implement `Clone` for
  it.
- `PublishedDomReader` is cloneable and can be sent to MTS.
- Both sides share only the `ArcSwap`, never a mutable `Dom` or a DOM lock.
- The snapshot and its `ChangeSet` are fields of the same `PublishedDom` stored
  by one `ArcSwap`. Do not put them in separate atomics.
- `DomSnapshot` keeps its `Dom` private and exposes read-only query methods only.
  It must not expose `&mut Dom`, mutation methods, or a method that consumes it
  back into a mutable staging DOM.
- `PublishedDom` exposes read-only `snapshot()`, `changes()`, and `revision()`
  accessors.

Including source and target revisions in `FullRebuild` now makes skipped
revision handling explicit and leaves a compatible place for Problem 3's
incremental batches. Enforce that `to_revision == snapshot.revision()` when a
publication is constructed.

## Read-only snapshot API

Delegate the queries needed by future MTS consumers without implementing
`Deref<Target = Dom>`:

- `revision`, `root`, and `contains`;
- `node`, `kind`, `element`, and `text`;
- `attribute`, `parent`, and `children`;
- pre-order iteration of the reachable app tree.

The snapshot can contain the complete cloned arena, including detached nodes.
Layout and rendering will traverse only the tree reachable from `root`. Keeping
the complete arena is the simplest first implementation and preserves all
`NodeId` generations exactly.

Re-export the read-only publication types from
`crates/burokku/src/ui/elements.rs` with the narrowest visibility needed by the
future host. Keep construction and swapping crate-private so application code
cannot publish arbitrary revisions.

## Publisher and checkpoint behavior

Add a constructor that receives the initial staging `Dom` and returns the
single writer plus a cloneable reader. Initialize `ArcSwap` with an immutable
baseline snapshot of the initial app-only DOM. Baseline initialization does not
send a redraw notification.

Add a checkpoint method with this behavior:

1. Read `staging.revision()`.
2. Return `None` immediately if it equals `last_published_revision`.
3. Clone staging into a private `DomSnapshot`.
4. Build `ChangeSet::FullRebuild` from the previous published revision to the
   snapshot revision.
5. Build one `Arc<PublishedDom>` containing both values.
6. Store that `Arc` in `ArcSwap`.
7. Update `last_published_revision`.
8. Notify MTS only after the store succeeds.
9. Return the published revision (or a small commit result) for diagnostics and
   tests.

Use `Dom::revision()` as the initial dirty detector rather than adding a second
per-mutation flag. This correctly coalesces all effective mutations in one task
into one publication. A sequence that mutates and then restores the same value
still publishes because mutations occurred and the DOM revision advanced.

The future JavaScript DOM plugin from Problem 4 should own both the staging
`Dom` and `DomPublisher`, then delegate its existing
`runtime::Plugin::checkpoint` callback to this checkpoint method. No runtime
scheduler change is needed for Problem 2 because checkpoint timing is already
implemented and tested in `crates/runtime/src/event_loop.rs`.

## Redraw notification boundary

Define a small `Send + Sync` notification interface (a trait or stored closure)
for `DomPublisher`. It should report that a newer committed revision is
available; it must not perform layout or read the staging DOM.

The production notifier should wake the native event loop through a
thread-safe signal such as `winit::EventLoopProxy`. MTS can then load the latest
publication and call `Window::request_redraw` on the UI thread. Do not call the
macOS window method directly from BTS. Multiple wakes/commits may be coalesced;
MTS is allowed to render only the latest complete revision.

The native application host and window ownership are not present yet, so this
problem should establish and test the notifier contract. The concrete
`EventLoopProxy`/window hookup belongs with Problems 10 and 11.

## MTS frame consumption contract

`PublishedDomReader::load()` should use `ArcSwap::load_full()` and return an
owned `Arc<PublishedDom>`. The frame code must:

1. load once at frame start;
2. retain that exact `Arc` through reconciliation, layout, hit testing, scene
   construction, and presentation;
3. record the revision represented by the frame;
4. load again only for a later frame.

Do not expose an API that repeatedly loads individual nodes from `ArcSwap`, as
that could mix revisions inside one frame. No mutex or guard should survive the
load operation.

## Implementation steps

1. **Define immutable publication types**
   - Replace the empty `DomSnapshot` placeholder.
   - Add `ChangeSet::FullRebuild`, `PublishedDom`, reader, publisher, and the
     notifier abstraction.
   - Add read-only accessors and useful `Debug` implementations.

2. **Connect publication to `Dom` safely**
   - Add a crate-private snapshot constructor or equivalent conversion from
     `&Dom`.
   - Keep all snapshot internals private and preserve `NodeId` values by cloning
     the existing slot map.
   - Re-export only the APIs required by other `burokku` modules.

3. **Implement dirty checkpoint publication**
   - Track the last published DOM revision in the non-cloneable writer.
   - Skip unchanged checkpoints.
   - Store `PublishedDom` atomically before notifying MTS.
   - Ensure one checkpoint produces at most one publication and one
     notification.

4. **Document the integration point**
   - Add a short comment/example showing that the future BTS DOM plugin calls
     the publisher from `Plugin::checkpoint`.
   - Document that the reader's returned `Arc` is frame-scoped.
   - Do not implement the JavaScript facade, Taffy reconciliation, or native
     window lifecycle in this change.

5. **Add focused tests**
   - Put unit tests beside `publication.rs`; add an integration test only if
     crate-private access becomes awkward.
   - Run formatting, the `burokku` test suite, and then the workspace suite.

## Required tests

### Publication semantics

- The initial reader contains revision `0`, the permanent app root, and a
  full-rebuild baseline without sending a notification.
- A checkpoint with no mutation does not swap or notify.
- Existing no-op mutations (same text/style/attribute, detaching an already
  detached node, or an unchanged child position) do not publish.
- Several effective mutations before one checkpoint produce exactly one
  publication and one notification.
- A failed/invalid mutation followed by no effective mutation does not publish.
- Successful mutations made before an error are still published at the next
  checkpoint.

### Snapshot correctness

- A reader loaded before publication continues to show the complete old tree
  after staging changes and after a newer revision is stored.
- A later load sees the complete new tree, including final parent/child order,
  style, attributes, and text.
- Node IDs are identical across revisions for nodes that were moved or updated.
- Removed IDs stay absent/stale, and slot reuse cannot make an old ID identify a
  replacement node.
- `ChangeSet::FullRebuild.to_revision` always equals the enclosed snapshot
  revision.

### Atomicity and concurrency

- A reader racing with a writer observes either the old `Arc<PublishedDom>` or
  the new one, never a new marker with an old snapshot (or the reverse).
- Holding an old frame `Arc` does not block staging mutations or a later store.
- Publishing while an old snapshot is retained leaves the old snapshot
  unchanged, exercising the existing `Arc::make_mut` copy-on-write behavior.
- The notification callback can immediately load the target revision, proving
  that notification occurs after the `ArcSwap` store.
- Add compile-time assertions that the reader/publication types are `Send +
  Sync`.

### Revision coalescing

- If BTS publishes multiple checkpoints before MTS loads again, MTS can load
  the latest complete revision and safely use `FullRebuild` even though it
  skipped intermediate revisions.

## Commands

```bash
cargo fmt --all -- --check
cargo test -p burokku
cargo test --workspace
```

If formatting changes are needed, run `cargo fmt --all` before the checks.

## Acceptance criteria

- `publication.rs` no longer contains a placeholder.
- Effective staging mutations are published only at an explicit checkpoint.
- Unchanged checkpoints do not publish or request redraw.
- One atomically loaded `Arc<PublishedDom>` always contains a matching immutable
  snapshot and full-rebuild marker.
- Stable `NodeId` values survive snapshot creation and later revisions.
- MTS can retain one revision for an entire frame without a DOM lock.
- BTS can continue mutating staging while MTS retains an older revision.
- A post-store notification is emitted for each committed checkpoint.
- The tests above pass without implementing incremental `ChangeSet` discovery
  or the JavaScript facade.
