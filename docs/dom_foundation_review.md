# DOM foundation: current problems

## Scope

This review lists the current implementation problems in the working tree.

The agreed node and mount-root contract is defined in
`docs/dom_node_model.md`:

```text
Node
├── AppNode
├── TextNode
└── Element
```

The host-created `globalThis.app: AppNode` is the permanent root. It is not an
element. Scripts create detached nodes exclusively through
`app.createElement(...)` and `app.createTextNode(...)`, then mount a `Window`
under `app`.

## 1. The TypeScript runtime contract does not match the AppNode contract

`packages/runtime/src/index.ts` still assumes browser-specific element types,
exports a standalone `createElement` helper, and delegates creation to a browser
global factory. That conflicts with the agreed Burokku-native API.

Required changes:

- define Burokku-native `Node`, `AppNode`, `TextNode`, and `Element` interfaces;
- declare `globalThis.app: AppNode`;
- put `createElement` and `createTextNode` on `AppNode`;
- remove browser-specific element assumptions from `BurokkuElement` and
  `setStyles`;
- make framework typings map supported tags to Burokku element types;
- keep `AppNode` out of `BurokkuTagName` because scripts cannot create it.

## 2. Immutable DOM publication is absent

`crates/burokku/src/ui/elements/publication.rs` contains only an empty
`DomSnapshot` placeholder. `arc-swap` is present but unused.

The background runtime needs a mutable staging DOM, while the main thread needs
an immutable committed view. Layout and rendering must never observe a partial
JavaScript update.

Required flow:

```text
BTS staging Dom
    -> macrotask and ready microtasks complete
    -> runtime checkpoint
    -> immutable PublishedDom { snapshot, changes }
    -> ArcSwap publication
    -> redraw request
    -> MTS layout/render reconciliation
```

The initial implementation may publish `ChangeSet::FullRebuild`. It must still:

- publish only after a real mutation;
- publish the snapshot and its change marker atomically;
- keep snapshot mutation APIs inaccessible;
- preserve `NodeId` values across revisions;
- let the main thread retain one complete revision for an entire frame;
- avoid holding a DOM lock during layout or rendering.

Cloning the current `SlotMap<NodeId, Arc<Node>>` is acceptable for the first
correct implementation even though it is O(arena size).

## 3. Change discovery is incomplete

`NodeRevisions` records structure, style, and content revisions per node, but a
consumer must already visit a node to discover that its revision changed.
Removed nodes cannot be discovered from the new snapshot at all.

A full rebuild on each publication is a valid initial fallback. Incremental
reconciliation later requires a bounded change batch containing:

```text
inserted
moved
removed
layout_dirty
paint_dirty
text_dirty
attributes_dirty
```

The change batch must identify its source and target revisions. If the main
thread skips a required revision or the batch is unavailable, it must fall back
to a full rebuild.

The current `style` and `content` revisions are also too broad for efficient
layout, paint, text, and accessibility caches. They should be split when those
incremental caches are introduced.

## 4. JavaScript node facade

**Status: implemented in the current working tree.** The BTS-only `DomPlugin`
now exposes the native DOM arena to QuickJS without installing a browser
`document`.

The facade provides:

- the permanent `globalThis.app` wrapper for the existing `NodeKind::App` root;
- `app.createElement(tag)` and `app.createTextNode(data)`;
- `Node`, `AppNode`, `TextNode`, and `Element` behavior;
- a stable `NodeId -> live JS wrapper` identity cache so repeated access
  preserves `===` identity;
- `parentNode`, `childNodes`, `firstChild`, `nextSibling`, and connectedness;
- `appendChild`, `insertBefore`, `removeChild`, and replacement operations;
- `textContent`, `nodeValue`, and `TextNode.data`;
- attributes and a Burokku-native style declaration API;
- listener registration and removal;
- conversion of `DomError` and `StyleError` into useful JavaScript exceptions;
- a checkpoint hook that commits dirty staging state through the publisher.

DOM mutations must update staging state synchronously so JavaScript can read its
own writes. Publication remains deferred until the runtime checkpoint.

React and Solid integration tests are required because their renderers may rely
on additional structural node behavior beyond basic insertion methods.

## 5. Detached-node reclamation

**Status: implemented in the current working tree.** Factory-created detached
nodes are tracked through weak canonical wrappers and reclaimed by a
wrapper-aware mark/sweep pass at runtime checkpoints.

The facade distinguishes:

- an attached node retained by the app tree;
- a detached node retained by a live wrapper;
- a detached subtree containing a live descendant wrapper;
- a removed subtree whose wrappers must remain valid;
- a genuinely unreachable detached subtree that can be reclaimed.

The wrapper identity cache must not strongly retain every wrapper forever. From
its first implementation, use QuickJS finalizers, weak wrapper tracking, or a
host-side mark pass rooted at both the app tree and all live wrappers. Only a
genuinely reclaimed node should produce a stale `NodeId`.

Problems 4 and 5 are one implementation workstream. The detailed contract and
sequence are in `docs/dom_facade_lifetime_plan.md`.

## 6. Text DOM behavior, shaping, and measurement are incomplete

The current foundation has raw `NodeKind::Text`, styled `Element::Text`,
validated typography, inherited `ComputedTextStyle`, native/JavaScript text
accessors, and a strict text child model. Raw text nodes can be attached only
beneath `Element::Text`; text elements may nest recursively. Non-text
`textContent` assignment is rejected without mutation.

It does not yet have a working layout and paint pipeline. Missing work includes:

- collecting nested styled text into inherited runs;
- Parley shaping and glyph layout;
- a Taffy measurement callback for paragraph leaves;
- invalidating an enclosing paragraph when descendant text or style changes;
- caching shaped paragraphs by revision, width constraint, and font state;
- painting the exact shaped layout used for measurement.

An outer styled text element should own one measured Taffy leaf. Nested styled
text elements are runs within that paragraph and must not become independent
Taffy layout nodes. There is no standalone raw-text Taffy leaf fallback for an
invalid non-text parent.

The detailed implementation contract is in `docs/text_rendering_plan.md`.

## 7. Taffy reconciliation

**Status: initial full-rebuild implementation completed.** The MTS-only layout
engine in `crates/burokku/src/ui/layout/` uses Taffy 0.11's low-level trait API
rather than maintaining a mutable `TaffyTree` mirror.

It now provides:

- a generation-safe `DOM NodeId -> LayoutId -> Taffy NodeId` mapping;
- a validated, revision-scoped `LayoutTopology` derived from one retained
  immutable `PublishedDom`;
- full rebuilding for creation, removal, DOM reparenting, child order, and
  layout-style changes;
- App omission, detached-node cleanup, and Window-root viewport constraints;
- block, flex, grid, empty-box, and measured paragraph-leaf dispatch;
- flattening of nested text descendants into one paragraph measurement input;
- an injectable fallible `TextMeasurer` boundary with first-baseline support;
- complete-output Taffy caching that preserves measurement baselines;
- unrounded logical computed boxes and atomic replacement after success;
- a derived-topology boundary that can later represent positioned containing
  blocks separately from DOM and paint/stacking relationships.

Remaining integration is tracked by the adjacent workstreams: Parley implements
the real text measurer in Problem 6, the native host supplies live viewports in
Problem 10, and a bounded incremental `ChangeSet` can replace full rebuilding
after Problem 3 defines its protocol.

The detailed design and follow-up stages are in
`docs/dom_problem_7_taffy_trait_plan.md`.

## 8. Rendering and presentation are missing

Vello dependencies exist, but there is no scene construction or presentation
pipeline.

Required work:

- convert computed Taffy layout and paint data into a Vello scene;
- render backgrounds and shaped text;
- separate layout invalidation from paint invalidation;
- cache paint data without mixing revisions;
- retain one committed snapshot through layout and scene construction;
- track the DOM revision represented by the presented frame;
- request redraw after a new committed revision.

## 9. Events are missing

There is no complete path from native input to a JavaScript listener.

Required work:

- hit-test against the presented layout;
- target events by `NodeId` and presented revision;
- maintain listener registration for live wrappers;
- forward events through the bounded runtime macrotask queue;
- define bubbling, cancellation, and listener-removal behavior for the supported
  event subset;
- reject or safely handle events targeting stale revisions or reclaimed nodes.

At minimum, the current application contract requires click registration and
dispatch.

## 10. Native Window lifecycle integration is missing

The agreed tree permits a script/framework-created `Window` under the permanent
app root, but no implementation connects that element to a native window.

The host must define and implement:

- `DOM NodeId -> native WindowId` ownership;
- native creation when a `Window` is committed under `app`;
- resize propagation into viewport constraints and redraw;
- removal and replacement behavior during framework reconciliation;
- native close behavior and JavaScript notification;
- cleanup of GPU surfaces, Taffy state, and event targets;
- shutdown behavior after the final native window closes.

The current tree allows one attached window. Multi-window behavior should not be
claimed until the tree and lifecycle contracts explicitly support it.

## 11. End-to-end application integration is absent

The crate exposes the tree arena and style model, but it does not yet assemble
the native event loop, dual runtimes, staging DOM plugin, publisher, reconciler,
renderer, and event bridge into a usable application host.

End-to-end tests must eventually verify this complete flow:

```text
JavaScript app factory and mutations
    -> checkpoint publication
    -> Taffy and Parley layout
    -> Vello scene and presentation
    -> native input hit testing
    -> JavaScript event dispatch
```

## Implementation order

1. Align `packages/runtime` with the `AppNode` factory contract.
2. Implement immutable snapshot publication with a full-rebuild change marker.
3. Implement the JavaScript node facade, synchronous staging DOM, weak wrapper
   identity, and detached-node reclamation as one workstream.
4. Connect committed `Window` elements to native window lifecycle.
5. Implement Taffy reconciliation and Parley text measurement.
6. Implement Vello painting, presentation revisions, hit testing, and events.
7. Assemble the application host and add end-to-end React/Solid tests.
