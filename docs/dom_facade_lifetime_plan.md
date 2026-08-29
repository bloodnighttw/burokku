# JavaScript DOM facade and detached-node lifetime implementation plan

> Historical implementation plan. Its cross-thread publication details are superseded by [`dom_layout_colocation_plan.md`](dom_layout_colocation_plan.md).

This plan addresses **Problem 4: The JavaScript node facade is missing** and
**Problem 5: Detached-node reclamation is undefined** from
`docs/dom_foundation_review.md` as one implementation workstream.

The facade and lifetime model must be designed together. A facade backed by a
strong `NodeId -> JavaScript object` cache would preserve identity but leak every
node ever wrapped. A facade without wrapper-aware native reachability could
instead reclaim detached nodes that JavaScript can still access. The first
usable vertical slice must therefore include both canonical wrappers and weak
lifetime tracking; reclamation is not a later retrofit.

## Status

The facade and lifetime machinery are implemented in the current working tree.
The native plugin and lifetime logic live under
`crates/burokku/src/ui/dom_plugin/`, and the private facade bootstrap is
`crates/burokku/src/ui/scripts/dom_facade.js`. The strict text-placement rule is
also implemented: raw nodes attach only beneath recursively nestable text
elements.

## Current foundation to reuse

The current rewrite already provides:

- one mutable `Dom` arena with a permanent `NodeKind::App` root;
- generation-checked `NodeId` values backed by `SlotMap`;
- detached creation through `Dom::create_element` and `Dom::create_text`;
- synchronous parent/child, attribute, style, and raw-text mutations;
- immutable `PublishedDom` revisions and `PublishedDomReader`;
- a single-writer `DomPublisher` with full-rebuild change markers;
- `runtime::Plugin::checkpoint`, called after one macrotask and all ready
  QuickJS jobs;
- a full QuickJS context, including `WeakRef` and `FinalizationRegistry`.

The implementation must use these current APIs and contracts. It must not copy
or revive a previous DOM bridge.

## Goals

1. Expose the native arena through the agreed object-oriented JavaScript node
   model.
2. Install the permanent host-created `globalThis.app: AppNode` and no browser
   `document` factory.
3. Apply JavaScript mutations synchronously to BTS staging state so scripts can
   read their own writes.
4. Return the same JavaScript object for repeated access to the same live
   `NodeId`.
5. Keep detached components alive while any wrapper in that component remains
   live.
6. Reclaim a detached component only after it is unreachable from both the app
   tree and every live wrapper.
7. Run reclamation and immutable publication at the runtime checkpoint, never
   from a QuickJS finalizer callback.
8. Preserve generation safety so a reclaimed `NodeId` can never identify a
   replacement allocation.
9. Validate the facade against React and Solid renderers, including moves,
   replacement, text updates, and unmounting.

## Non-goals for this workstream

- Taffy reconciliation, Parley shaping, Vello rendering, or native windows.
- Native input delivery, hit testing, bubbling, or event cancellation. This
  work adds listener registration/removal only; complete dispatch is Problem 9.
- Browser globals such as `document`, `HTMLElement`, or CSSOM compatibility.
- Layout-reading APIs such as `getBoundingClientRect()`.
- Mutation observers, ranges, selection, shadow DOM, HTML parsing, namespaces,
  or arbitrary custom elements.
- Incremental `ChangeSet` construction. Publications may continue using
  `ChangeSet::FullRebuild`.
- Immediate or deterministic production garbage collection. Reclamation is
  eventual and follows QuickJS collection/finalization.

## Required ownership boundary

The DOM remains owned by the background runtime:

```text
BTS QuickJS application/framework
    -> JavaScript facade and private weak wrapper registry
    -> private synchronous native bindings
    -> mutable staging Dom
    -> checkpoint lifetime sweep
    -> DomPublisher checkpoint
    -> immutable PublishedDom
    -> MTS PublishedDomReader
```

Add a BTS-only plugin, with names adjusted to repository conventions if needed:

```rust
pub(crate) struct DomPlugin {
    state: Arc<Mutex<DomPluginState>>,
    publisher: DomPublisher,
}

struct DomPluginState {
    staging: Dom,
    live_wrappers: HashMap<NodeId, usize>,
}
```

The plugin constructor should create the staging `Dom` and `DomPublisher`, then
return the plugin and its `PublishedDomReader`. Only the Burokku host may own
these capabilities.

`Plugin::install(&self, ctx)` needs shared interior state because installed
native callbacks and `Plugin::checkpoint(&mut self)` access the same staging
DOM. The mutex is BTS-local coordination, not an MTS DOM lock:

- lock for one native query or mutation only;
- convert inputs to owned Rust values before locking;
- return owned values and release the lock before creating more JavaScript
  objects or calling JavaScript;
- never hold the lock across `.await`, layout, rendering, or native event-loop
  work;
- do not expose the mutex, staging `Dom`, or publisher outside the plugin.

Reject installation into the main runtime. A standalone role may be supported
only by an explicit test constructor; production installs the plugin in BTS.

## Module layout

A suitable initial layout is:

```text
crates/burokku/src/ui/
├── dom_plugin.rs
├── dom_plugin/
│   ├── bindings.rs
│   ├── errors.rs
│   └── lifetime.rs
├── scripts/
│   └── dom_facade.js
└── elements.rs
```

Responsibilities:

- `dom_plugin.rs`: ownership, installation, and checkpoint ordering;
- `bindings.rs`: private QuickJS-to-`Dom` operations;
- `errors.rs`: one mapping from native errors to JavaScript exceptions;
- `lifetime.rs`: live-wrapper accounting and detached-component mark/sweep;
- `dom_facade.js`: public classes, private handles, identity interning, style
  declarations, and listener registration;
- `elements.rs`: native operations that must be atomic independently of
  JavaScript.

Keep the embedded facade script small and test it as source. Do not publish the
private native binding object on `globalThis`.

## JavaScript facade contract

Expose the hierarchy defined by `docs/dom_node_model.md`:

```text
Node
├── AppNode
├── TextNode
└── Element
    ├── Window
    ├── Div
    ├── Flex
    ├── Grid
    └── TextElement
```

The tag-specific element classes may initially add no behavior beyond
`Element`, but their prototypes must inherit correctly if they are exposed.
Constructors are host-only: calling them directly from application JavaScript
must throw an `Illegal constructor` `TypeError`.

### `Node`

Implement at least:

- `parentNode`;
- `childNodes` as a newly created snapshot array for each read;
- `firstChild`, `lastChild`, `nextSibling`, and `previousSibling`;
- `isConnected`;
- `appendChild(child)`;
- `insertBefore(child, referenceChildOrNull)`;
- `removeChild(child)`;
- `replaceChild(newChild, oldChild)`;
- `contains(other)`;
- `textContent` getter and setter;
- `nodeValue` getter and setter;
- `addEventListener(type, callback)` and
  `removeEventListener(type, callback)` for the supported listener subset.

Mutation methods return the affected wrapper in the normal DOM shape:
`appendChild` and `insertBefore` return the inserted child, `removeChild`
returns the detached child, and `replaceChild` returns the replaced child.

`childNodes` is intentionally a snapshot array, not a live browser `NodeList`.
The shared JavaScript prototype may implement text setters for dispatch, but
`textContent` assignment succeeds only for `TextNode` and `TextElement`;
non-text nodes receive the native error. Generic `Node` text properties are
therefore read-only in TypeScript. Do not expose unsupported browser behavior
through the TypeScript contract.

### `AppNode`

`globalThis.app` is the one wrapper for `Dom::root()` and is installed as a
non-writable, non-configurable global. It provides:

```ts
createElement(tag: BurokkuTagName): Element;
createTextNode(data: string): TextNode;
```

Factory results are detached but already protected by a live wrapper before
control returns to application JavaScript. `AppNode` is not an `Element`, has no
attributes or style, cannot be created by script, and continues to accept only
one `Window` child through native relationship validation.

### `TextNode`

`TextNode.data`, `textContent`, and `nodeValue` all read the same raw text. Their
setters update that existing `NodeId` synchronously.

### `Element`

Implement:

- immutable `localName`;
- `getAttribute`, `hasAttribute`, `setAttribute`, and `removeAttribute`;
- a stable `style` object for the lifetime of the element wrapper.

The initial Burokku-native style declaration supports:

```ts
interface BurokkuStyleDeclaration {
  supportsProperty(name: string): boolean;
  setProperty(name: string, value: string): void;
  removeProperty(name: string): void;
}
```

Use the kebab-case property names already consumed by native style parsers.
Direct CSSOM property assignment and browser-complete `getPropertyValue()` are
not part of the initial contract. A style declaration must retain its owner
wrapper strongly; an escaped `element.style` object must therefore keep the
native element alive.

### Text semantics

Add the native text operations specified by `docs/text_rendering_plan.md`:

```rust
Dom::text_content(id) -> Result<String, DomError>
Dom::set_text_content(id, value) -> Result<bool, DomError>
Dom::set_text(text_id, value) -> Result<bool, DomError>
```

Required behavior:

- a raw text node may be attached only beneath an `Element::Text`;
- text elements may contain raw text and nested text elements recursively;
- `Window`, `Div`, `Flex`, and `Grid` accept explicit text elements but reject
  raw text children atomically;
- a raw text getter returns its data;
- an element getter concatenates descendant raw text in tree order;
- a raw text setter updates the existing node;
- a text-element setter detaches all existing children and attaches one new raw
  text child containing the assigned value;
- `textContent` assignment on `AppNode` and non-text elements is rejected
  without mutation;
- replaced children are detached, not immediately destroyed;
- assigning the same effective value is a no-op when no structural change is
  needed;
- `nodeValue` is `null` and its setter is a no-op for `AppNode` and elements.

Use an iterative traversal or a checked depth limit for descendant text
collection.

### Listener storage boundary

Listener registration must not create a permanent native root. For this phase,
keep listener records in JavaScript state owned by the canonical wrapper and
trace them normally with that wrapper. Dropping an unreachable detached wrapper
must also make its listener cycle collectable.

Do not store every callback in an unconditional native
`HashMap<NodeId, Persistent<Function>>`; such a map would keep callbacks that
close over wrappers alive and defeat detached-node reclamation. Problem 9 may
add a private dispatcher or ephemeron-like listener index, but it must preserve
the reachability rules in this plan.

## Private native handles

A wrapper stores a private opaque encoding of its `NodeId`. Never expose a raw
arena index and never encode the complete generation-checked ID as a JavaScript
`number`, which cannot represent every `u64` exactly. Use a lossless private
string or `BigInt` token.

The facade bootstrap should receive a private native binding object as a
function argument and close over it. Keep both the binding object and wrapper
constructor key out of global properties. Every native operation must still
validate the decoded generation because queued work and bugs can present stale
handles.

No JavaScript wrapper may retain a Rust reference or pointer into `Dom`; it
retains only the opaque `NodeId` token.

## Canonical weak wrapper registry

Implement interning in `dom_facade.js` with:

- `Map<NodeToken, { generation, reference: WeakRef<Node> }>`;
- one monotonically increasing wrapper-cache generation;
- `FinalizationRegistry` held values containing the node token and cache
  generation;
- native live-wrapper acquire/release accounting.

Conceptually:

```text
wrap(node_id):
    return null for no node
    look up weak entry
    if deref() returns a wrapper, return it
    query and validate native node kind
    construct the matching private wrapper class
    acquire one native wrapper root
    install WeakRef and finalization registration
    return the wrapper

finalize({ node_id, generation }):
    remove the map entry only if its generation still matches
    release one native wrapper root
```

The generation check prevents a delayed finalizer for an older wrapper from
deleting a newer cache entry for the same still-valid node. Native root counts
must tolerate the short interval in which an old unreachable wrapper is
awaiting finalization while a new wrapper has already been created.

Acquisition and registration must be failure-safe. If wrapper construction or
registry insertion throws, roll back any acquired native root before exposing
an incomplete wrapper.

The finalization callback may update live-wrapper bookkeeping only. It must not:

- mutate parent/child relationships;
- remove arena entries;
- publish a snapshot;
- request redraw;
- invoke application callbacks.

QuickJS decides when finalizers run. A release observed after one checkpoint is
processed by the next checkpoint; immediate reclamation is not promised.

## Native reachability and reclamation

### Reachability rule

A native node is retained when either:

1. it is in the component rooted at the permanent app node; or
2. any JavaScript wrapper in its detached component is live.

A live wrapper retains its **entire detached component**, not just the wrapped
node and its descendants. From a descendant wrapper, JavaScript can traverse to
its parent and then to siblings, so reclaiming any other node in that component
would make observable traversal invalid.

The five required states therefore behave as follows:

| State | Result |
| --- | --- |
| Attached under `app` | Retained by the app tree |
| Detached node with a live wrapper | Its detached component is retained |
| Detached subtree with a live descendant wrapper | The complete detached component is retained |
| Removed subtree with any live wrapper | Wrappers and traversal remain valid |
| Detached component with no live wrappers | Eligible for checkpoint reclamation |

### Mark/sweep API

Add a crate-private batch operation, for example:

```rust
Dom::reclaim_unreachable_detached(
    live_wrappers: impl Iterator<Item = NodeId>,
) -> ReclaimReport
```

Use an iterative algorithm:

1. Mark the complete tree reachable from `Dom::root()`.
2. For each live wrapper not already in that tree:
   - validate that its `NodeId` still exists;
   - walk parent links to the detached component root;
   - mark that root and all descendants.
3. Identify every unmarked detached component root.
4. Remove each unmarked component from the `SlotMap`.
5. Advance the DOM revision once for the reclamation batch when at least one
   node was removed.
6. Return reclaimed roots/IDs for diagnostics and cleanup of future side
   tables.

The app root can never be swept. Invariant failures such as a live wrapper for a
missing node should fail loudly in debug/tests and become a safe stale-handle
error in production.

JavaScript structural removal must call `detach`-style operations, not
`remove_subtree`. Reserve permanent arena deletion for the lifetime sweep (and
focused arena tests). Consider reducing `remove_subtree` visibility once all
callers follow this rule.

### Generation safety

Reclamation removes `SlotMap` entries so their generations become stale. A
later allocation may reuse storage but cannot compare equal to the reclaimed
`NodeId`. Every binding must map a stale handle to a useful JavaScript exception
rather than panic or access a replacement node.

An older immutable `PublishedDom` may continue to contain a reclaimed node while
an MTS frame retains that older publication. This is valid: the old frame owns a
complete old revision, while a later publication reflects reclamation.

## Native DOM operations needed by the facade

Add atomic Rust helpers instead of reproducing relationship logic in
JavaScript:

```rust
Dom::element_tag(id)
Dom::is_connected(id)
Dom::first_child(id)
Dom::last_child(id)
Dom::next_sibling(id)
Dom::previous_sibling(id)
Dom::contains_node(ancestor, descendant)
Dom::insert_before(parent, child, reference)
Dom::remove_child(parent, child)
Dom::replace_child(parent, new_child, old_child)
```

Also add one checked conversion from supported tag names to default `Element`
variants.

Important mutation rules:

- `insertBefore` verifies that a non-null reference is a direct child before
  changing anything.
- Moving a child within the same parent calculates the index against the final
  child list.
- Inserting a node before itself is a no-op.
- `removeChild` rejects a node that is not a direct child.
- `replaceChild` validates and applies the final relationship atomically. It
  must support replacing the sole `Window` under `app` without transiently
  violating the one-window rule.
- The replaced node becomes detached and remains valid while wrapped.
- Invalid relationships, cycles, stale IDs, and wrong node kinds leave staging
  state and its revision unchanged.

Tag creation accepts only the names in `BurokkuTagName`: `window`, `div`,
`flex`, `grid`, and `text`. Unknown names fail without allocating a node.

## Error mapping

Centralize native-to-JavaScript conversion. Do not return
`rquickjs::Error::Unknown` for expected DOM failures.

Install a small Burokku-native error type, or equivalent structured errors, with
stable names/codes. Suggested mapping:

| Native condition | JavaScript error |
| --- | --- |
| Unknown tag or invalid argument type | `TypeError` |
| Stale/reclaimed `NodeId` | `InvalidStateError` |
| Wrong native node kind | `InvalidNodeTypeError` |
| Invalid parent/child kind, app-root move, cycle, second window | `HierarchyRequestError` |
| Missing reference/direct child | `NotFoundError` |
| Unsupported style property | `UnsupportedStylePropertyError` |
| Invalid style value | `InvalidStyleValueError` |

Messages should include the operation and safe contextual information, but not
expose implementation pointers. Failed operations must not partially mutate the
DOM.

## Checkpoint and publication order

`DomPlugin::checkpoint` performs work in this order:

```text
QuickJS macrotask finishes
    -> all ready microtasks/finalization jobs drain
    -> observe final wrapper releases
    -> mark and reclaim unreachable detached components
    -> DomPublisher::checkpoint(staging)
    -> atomic PublishedDom store when revision changed
    -> notify MTS after the store
```

Consequences:

- all successful JavaScript mutations in the task remain synchronously visible
  to later JavaScript in that task;
- an exception does not roll back successful earlier mutations;
- no-op tasks and checkpoints do not publish;
- mutations and reclamation in one turn produce at most one publication and one
  notification;
- a reclamation-only checkpoint may publish a new revision for correctness,
  even though the reclaimed component was detached;
- the checkpoint remains context-free and never executes JavaScript.

## TypeScript contract

Problem 1 should land before or with the facade integration. The declarations
in `packages/runtime/src/index.ts` must describe Burokku objects, not browser
`HTMLElement` values.

At minimum, declare the supported `Node`, `AppNode`, `TextNode`, `Element`, tag
mapping, and `BurokkuStyleDeclaration`. Update `setStyles` to accept a Burokku
`Element` and call its native style declaration. Remove the standalone browser
`createElement` helper; creation belongs to `app`.

Add compile-only tests proving:

- `app.createElement("div")` returns the mapped element type;
- `app.createTextNode(...)` returns `TextNode`;
- generic `Node` and non-text elements expose read-only text properties, while
  `TextNode` and `TextElement` expose the supported writable properties;
- child mutation types accept `textElement.appendChild(textNode)` and reject
  `div.appendChild(textNode)`;
- `AppNode` is not a `BurokkuTagName` or an `Element`;
- browser-only `HTMLElement` members are not accidentally promised;
- unsupported tags and style properties fail type checking.

## React and Solid integration strategy

Use framework-native custom renderer APIs rather than adding a fake browser
`document`:

- React: a small `react-reconciler` host config backed by `AppNode` and `Node`
  operations;
- Solid: `solid-js/universal` `createRenderer` operations backed by the same
  facade.

Bundle deterministic test fixtures to plain JavaScript and evaluate them in a
BTS runtime with `DomPlugin`. Inspect staging state during a task where needed,
and inspect `PublishedDomReader` after checkpoints.

Each framework fixture must cover:

1. initial creation and mounting of one `Window`;
2. raw text inside explicit, recursively nestable text elements;
3. rejection of a direct raw child beneath a non-text element;
4. attribute and style updates;
5. keyed sibling reordering without losing `NodeId` identity;
6. text updates through both text nodes and text-element `textContent`;
7. element replacement and unmounting;
8. retained wrappers remaining valid after removal;
9. one publication per completed render turn rather than per mutation.

Only add facade behavior actually required by the agreed contract or observed
renderer needs. Record any extra structural method in both this document and
the TypeScript API; do not silently emulate the entire browser DOM.

## Implementation sequence

### Phase 1: Prove the lifetime primitive

- Add a focused rquickjs test proving `WeakRef` and `FinalizationRegistry` work
  in the context configuration used by `runtime`.
- Force QuickJS GC from a Rust test harness, drain pending jobs, and verify one
  acquire produces one release.
- Verify a delayed old finalizer cannot remove a newer cache generation.
- Keep finalization callbacks limited to root bookkeeping.

This phase is a gate: do not build the facade around a temporary strong cache.

### Phase 2: Add native facade semantics

- Add checked tag-to-default-element creation.
- Add traversal and connectedness helpers.
- Add atomic insert-before, direct-child removal, and replacement helpers.
- Add `text_content` and `set_text_content` semantics.
- Add batched detached-component mark/sweep.
- Add focused native tests before connecting QuickJS.

### Phase 3: Build a lifetime-safe vertical slice

- Add `DomPlugin` ownership and private bindings.
- Bootstrap the core `Node`, `AppNode`, `TextNode`, and `Element` prototypes.
- Install the permanent `app` wrapper.
- Implement `createElement`, `createTextNode`, `parentNode`, `childNodes`, and
  `appendChild`.
- Add weak interning, finalization generations, and live-wrapper accounting in
  the same change.
- Run reclamation before publication in the plugin checkpoint.

The vertical slice is complete only when repeated access preserves `===`, a
live detached wrapper survives GC/checkpoint, and an unreachable detached node
is reclaimed.

### Phase 4: Complete mutation and data APIs

- Add sibling traversal and connectedness.
- Add `insertBefore`, `removeChild`, `replaceChild`, and `contains`.
- Add text properties and replacement behavior.
- Add attributes and stable style declaration objects.
- Add listener registration/removal without creating native lifetime roots.
- Add centralized JavaScript exception mapping.

### Phase 5: Align TypeScript and package helpers

- Finish the Burokku-native interfaces and tag map.
- Change `setStyles` to the native style declaration.
- Remove browser factory/type assumptions.
- Add positive and negative compile tests.

### Phase 6: Add framework and checkpoint integration tests

- Add and bundle the React custom renderer fixture.
- Add and bundle the Solid universal renderer fixture.
- Test mount, update, move, replacement, unmount, and wrapper retention.
- Verify full-rebuild publication remains atomic and coalesced.

### Phase 7: Robustness and measurement

- Stress repeated detached creation, collection, and slot reuse.
- Add deep/wide component tests using iterative marking.
- Measure wrapper lookup, checkpoint sweep, and arena size over many cycles.
- Add counters for live wrappers, weak-cache entries, detached nodes, and
  reclaimed nodes in debug/test builds.
- Define a reasonable sweep strategy if scanning the complete arena at every
  checkpoint becomes measurable; preserve the same reachability semantics.

## Required tests

### Native DOM semantics

- Every supported tag creates the correct default `Element` variant.
- Unknown tags allocate nothing and do not advance the revision.
- Sibling traversal and `isConnected` are correct before and after moves.
- Same-parent moves and insert-before-self are correct no-ops.
- Invalid references and non-children leave the tree unchanged.
- Replacing the app's existing `Window` is atomic and valid.
- Replaced and removed children become detached rather than stale.
- Raw text is accepted only beneath text elements, which may nest recursively.
- Direct raw children beneath non-text elements are rejected without mutation.
- Text getters concatenate nested text in tree order.
- Text-element replacement detaches old children and handles no-ops; non-text
  `textContent` assignment is rejected.

### Wrapper identity and facade behavior

- `globalThis.app === globalThis.app` and its descriptor is permanent.
- Direct construction of every node class fails.
- Repeated parent, child, and sibling reads return the same live wrapper.
- Moving a native node does not change its wrapper identity.
- Core and tag-specific prototype relationships are correct.
- A stable style declaration retains and mutates its owner element.
- Wrong receiver kinds and stale handles produce the documented exceptions.
- Successful writes are immediately readable before checkpoint publication.

### Lifetime and reclamation

- A newly created detached node remains valid while its returned wrapper lives.
- Dropping the final wrapper permits reclamation after GC, job draining, and a
  checkpoint.
- A live descendant wrapper retains its parent, siblings, and complete detached
  component.
- A removed subtree remains traversable while any wrapper in it is live.
- Dropping all wrappers reclaims the complete detached component.
- An attached tree remains native-live even when no non-app wrappers remain.
- Wrapper/listener cycles do not become permanent native roots.
- A delayed finalizer cannot release a newer wrapper's only root.
- Repeated create/drop/GC/checkpoint cycles return the arena near its baseline
  size instead of growing without bound.
- A reclaimed ID stays stale after slot reuse.

### Checkpoint and publication

- Multiple synchronous facade mutations produce one publication.
- Mutations followed by a JavaScript exception are still published.
- A no-op checkpoint neither swaps nor notifies.
- Finalizer bookkeeping without eligible nodes does not publish.
- Reclamation and ordinary mutations in one checkpoint produce one complete
  snapshot and notification.
- A retained old `PublishedDom` remains immutable after staging reclamation.

### Framework integration

- React and Solid can mount under `app` without `document` or `HTMLElement`.
- Keyed reorders preserve native and JavaScript identity.
- Framework unmount detaches first; retained user wrappers stay valid.
- Once framework/user references disappear, detached native nodes are
  reclaimable.

## Commands

```bash
pnpm typecheck
pnpm test
cargo fmt --all -- --check
cargo test -p burokku
cargo test --workspace
```

Add a focused framework-fixture build command if those bundles live in a
separate workspace package, and run it before the Rust integration tests that
embed them.

## Completion criteria

Problems 4 and 5 are complete for the initial contract when:

- BTS installs a permanent `globalThis.app` over the existing app root;
- all supported node creation, traversal, mutation, text, attribute, style, and
  listener-registration APIs operate synchronously on staging DOM state;
- each live `NodeId` has one canonical observable wrapper;
- the identity cache contains weak, generation-safe entries rather than strong
  permanent JavaScript roots;
- app-connected nodes and detached components with live wrappers are retained;
- unreachable detached components are reclaimed only at checkpoints;
- reclaimed IDs become safely stale and cannot alias slot reuse;
- checkpoint publication remains atomic and coalesced;
- current React and Solid custom renderers pass mount/update/move/unmount tests;
- all TypeScript, Rust, GC-lifetime, and publication tests pass.
