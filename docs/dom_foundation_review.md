# DOM foundation review

## Scope

This review covers the current working tree only. Git history and pre-refactor
implementations were not consulted.

Primary implementation reviewed:

- `crates/burokku/src/ui/elements.rs`
- `crates/burokku/src/ui/elements/`
- the current TypeScript runtime contract and examples

## Summary

The current implementation is a useful tree arena, but it is not yet a usable
DOM pipeline. Stable handles, structural validation, detached construction,
and copy-on-write nodes are a good foundation. The main blockers are incorrect
layout defaults, an undefined document root, missing publication, missing
JavaScript bindings, incomplete text handling, and no layout/render/event
integration.

## Major findings

### 1. `Div` currently becomes a flex container

`Element::Div` is documented as a block element in
`crates/burokku/src/ui/elements.rs:38`, but
`CommonStyle::to_taffy_style()` leaves `display` at Taffy's default in
`crates/burokku/src/ui/elements/styles/common.rs:32-55`.

With the enabled Taffy features, the default display mode is `Flex`. This means
both `Div` and styled `Text`, which use `CommonStyle`, are converted into flex
containers.

Required changes:

- explicitly use `taffy::Display::Block` for `Div`;
- give styled text a dedicated layout and measurement path rather than treating
  it as a generic common-style container;
- add tests asserting the converted display mode for every element tag.

### 2. DOM publication is absent

`crates/burokku/src/ui/elements/publication.rs` contains only an empty duplicate
`Dom` declaration. The `arc-swap` dependency is currently unused.

There is no implementation for:

- an immutable `DomSnapshot`;
- a mutable staging DOM owner;
- dirty tracking;
- checkpoint commits;
- atomic snapshot publication;
- a mutation/change batch;
- redraw notification or presented-revision tracking.

Although cloning the current `Dom` shares individual `Arc<Node>` values, the
clone is still another publicly mutable `Dom`. Snapshot immutability is only a
convention, and cloning the `SlotMap` remains an O(arena-size) operation.

Required design:

```text
BTS staging Dom
    -> runtime checkpoint
    -> immutable committed DomSnapshot + ChangeSet
    -> ArcSwap publication
    -> MTS layout/render reconciliation
```

Only publish when the staging DOM is dirty, and publish once after a complete
JavaScript macrotask plus its microtasks.

### 3. The document and root-element contract is undefined

`Dom::new()` creates only an internal `App` node. `App` accepts only one
`Window`, but current consumers immediately access and style `document.body`
without creating a window.

Recommended bootstrap tree:

```text
App
└── Window       native host and viewport
    └── Body      regular Div exposed as document.body
```

This keeps native window state separate from content layout. It also allows the
existing examples to apply common properties such as padding and background
color to `document.body`.

The contract must define:

- whether `Window` is host-created or script-created;
- whether `window` belongs in `BurokkuTagName`;
- which node `document.body` references;
- whether removing or replacing the body/window is permitted;
- how native window size constrains the body layout root.

### 4. `WindowStyle` is internally inconsistent

`WindowStyle` stores `background_color` in
`crates/burokku/src/ui/elements/styles/window.rs:11-15`, and
`Element::background_color()` reads it. However, `supports_property`,
`set_property`, and `remove_property` only implement width and height.

Consequences:

- `background-color` cannot be assigned through the DOM style API;
- the field can only be changed by constructing and replacing the complete
  Rust element;
- mapping `document.body` directly to `Window` would make the examples' body
  padding and background styles fail.

Prefer a separate body element. If window paint remains supported, implement
its style operations consistently.

### 5. `NodeId` does not identify its owning DOM lineage

`NodeId` contains only SlotMap slot and generation information. Independent
fresh `Dom` instances can issue identical IDs. Passing an ID from one DOM to
another can therefore resolve to an unrelated node instead of producing
`DomError::NodeNotFound`.

This matters when handles are moved through runtime messages or when more than
one document can exist.

Possible solutions:

- include a stable document/lineage ID alongside the SlotMap key;
- wrap IDs in a handle carrying and validating its owner;
- make construction and all cross-thread APIs enforce a single DOM lineage.

Committed snapshots from the same lineage should intentionally preserve the
same node handles.

### 6. Grid item properties are attached to grid containers

`GridStyle` combines container properties with item properties such as `row`,
`column`, and `justify_self`. Only `Element::Grid` owns `GridStyle`.

As a result, a `Div`, `Flex`, or `Text` that is a child of a grid cannot receive
normal grid-item placement or `justify-self`. These properties describe an
item's relationship to its parent and must be available to every layout
element.

Additionally, grid row/column/template fields exist in Rust but are not handled
by the string style API.

Required changes:

- move grid-item placement and `justify-self` into a shared item-style section;
- keep template, auto-flow, gap, and item-alignment properties on grid
  containers;
- add parsing/removal support for every property exposed to TypeScript;
- either expose the remaining grid properties through TypeScript or remove
  unfinished public claims until implemented.

### 7. Text nodes cannot yet be laid out or rendered correctly

The tree distinguishes styled `Element::Text` from raw `NodeKind::Text`, which
is a useful representation for styled runs. However, the required behavior is
missing:

- a `TextStyle` containing font family, size, weight, color, line height, and
  wrapping behavior;
- inherited text style;
- text shaping and glyph layout;
- a Taffy leaf measurement callback;
- element-level `textContent` get/set semantics;
- concatenation of descendant text for getters;
- invalidation of an enclosing shaped-text cache when a descendant run changes.

The examples already require assignments such as:

```ts
const title = createElement("text");
title.textContent = "Click counter";
```

Setting `textContent` on an element should replace its children with one raw
text node, while setting `nodeValue`/`data` should update an existing raw text
node.

### 8. Revisions alone do not provide change discovery

`NodeRevisions` tracks structure, style, and content per node, but consumers
have no dirty-node set or mutation journal. After observing a new global DOM
revision, the main thread must traverse the entire reachable tree to discover
what changed.

Other issues:

- removed descendants are not returned as a removal set;
- layout and paint share one `style` revision;
- changing only `background-color` can force layout-style processing;
- arbitrary attributes such as `role` are classified as visual content;
- parent revisions do not summarize descendant text changes.

Recommended revision categories:

```text
structure
layout
paint
text
attributes/accessibility
```

Each commit should also carry a bounded `ChangeSet` containing inserted, moved,
removed, layout-dirty, paint-dirty, and text-dirty node IDs. A full rebuild can
remain the fallback when the change set is unavailable or too large.

### 9. Style parsing admits invalid numeric state

Current `f32` parsing accepts values such as `NaN` and infinity. Negative sizes,
negative gaps/padding, and negative flex factors are also not validated before
being placed in authoritative DOM state.

There are contract mismatches with `packages/runtime/src/index.ts`:

- TypeScript exposes `alignSelf: "auto"` and `justifySelf: "auto"`, but Rust
  attempts to parse them as concrete Taffy alignment values instead of mapping
  them to `None`;
- TypeScript dimensions use `px`, `%`, or `auto`, while `WindowStyle` accepts
  only `auto` or a unitless float;
- `BurokkuColor` is any string, while native parsing currently accepts only a
  subset of hexadecimal colors.

`Result<bool, DomError>` also conflates an unsupported property with an invalid
value. Use a typed error such as:

```rust
enum StyleError {
    NodeNotElement(NodeId),
    UnsupportedProperty(String),
    InvalidValue { property: String, value: String },
}
```

All accepted numeric values should be finite and satisfy property-specific
constraints.

### 10. Taffy conversion consumes authoritative style values

`Styles::to_taffy_style(self)` takes ownership of the style. A renderer reading
an immutable snapshot cannot move style values out of it, so it must clone them
first. This is especially expensive for `GridStyle`, which owns vectors and
strings.

Prefer conversion from `&self`, and cache converted/computed Taffy state using
the node's layout revision. The resulting Taffy style may still need owned
strings, but unchanged DOM styles should not be repeatedly cloned and parsed.

### 11. Detached-node ownership and garbage collection are undefined

`create_element` and `create_text` allocate detached nodes immediately. If a
JavaScript wrapper becomes unreachable before insertion, the arena retains the
node unless something explicitly calls `remove_subtree`.

The JS integration must distinguish:

- detaching a node from the document while keeping it valid for live JS
  references;
- permanently reclaiming an unreachable detached subtree;
- removing an attached subtree from rendering without invalidating wrappers;
- stale handles from genuinely reclaimed nodes.

QuickJS wrapper finalizers or an explicit host-side wrapper registry should
reclaim detached nodes only when no JS wrapper or tree relationship retains
them.

### 12. The JavaScript DOM facade is missing

QuickJS does not provide browser DOM classes. The project needs a DOM plugin
that creates and maintains at least:

- `document` and `document.body`;
- `Node`, `Element`, `HTMLElement`, and `Text` behavior;
- a stable `NodeId -> JS wrapper` identity cache so repeated access preserves
  `===` identity;
- `createElement` and `createTextNode`;
- `parentNode`, `childNodes`, `firstChild`, `nextSibling`, and connectedness;
- `appendChild`, `insertBefore`, `removeChild`, and replacement operations;
- `textContent`, `nodeValue`, and text data;
- attributes;
- `CSSStyleDeclaration` set/remove/read behavior;
- event listener registration and removal;
- conversion of `DomError` and style errors into useful JavaScript exceptions.

The implementation does not need to emulate the entire browser DOM, but the
supported contract must be explicit and match `packages/runtime`.

## Missing integration layers

### DOM plugin and mutation owner

The background runtime should normally own the mutable staging DOM. DOM methods
must update it synchronously so JavaScript can read its own writes. The plugin's
runtime checkpoint should publish one coherent revision after all ready
microtasks finish.

### Snapshot publisher

Implement immutable committed snapshots with `ArcSwap`. Publication must not
hold a DOM synchronization lock during layout or rendering. Coalesce multiple
mutations from one JavaScript task into one publication and one redraw request.

### Taffy reconciler

Maintain an MTS-only mapping:

```text
DOM NodeId -> Taffy NodeId
```

The reconciler must handle:

- creation and removal;
- reparenting and child order;
- layout-style revision changes;
- text measurement and dirty propagation;
- viewport constraints from the native window;
- cleanup of Taffy nodes absent from the committed DOM.

### Rendering

Convert computed layout plus paint/text data into a Vello scene. Paint caching
should be invalidated separately from layout. Rendering should always use one
complete committed DOM revision and its corresponding computed layout.

### Events

The main thread needs hit testing against the presented layout, a listener
registry, event targeting by `NodeId` and presented revision, and asynchronous
dispatch into the appropriate runtime through its bounded macrotask queue.

At minimum, the current counter example requires `click` registration and
dispatch.

### Lifecycle and native window integration

The host must define how the `Window` DOM node creates, configures, resizes, and
closes the native window, and how closing the final window shuts down both
runtimes.

## Recommended implementation order

1. **Correct the element/style model**
   - make `Div` block;
   - separate tag, common item style, container style, paint style, and text
     style;
   - move grid-item properties into shared item data;
   - validate style values and return typed errors.

2. **Define and bootstrap the document tree**
   - create `App -> Window -> Body(Div)`;
   - specify script permissions for window/body mutation;
   - add tag-name constructors and getters.

3. **Implement immutable publication**
   - staging DOM owner;
   - dirty flag and `ChangeSet`;
   - `DomSnapshot`;
   - `ArcSwap` publisher;
   - runtime-checkpoint commit.

4. **Implement the minimal JS DOM plugin**
   - wrapper identity and lifetime;
   - node traversal and mutation methods;
   - attributes, styles, and text content;
   - JavaScript error mapping.

5. **Implement Taffy reconciliation and text measurement**
   - stable Taffy-node mapping;
   - incremental structural/style changes;
   - shaping and measurement for text nodes.

6. **Implement paint, presentation, and events**
   - Vello scene construction;
   - layout/paint invalidation;
   - hit testing and click dispatch;
   - presented-revision tracking.

7. **Add end-to-end tests**
   - execute the TypeScript consumer contract through QuickJS;
   - publish at checkpoints;
   - reconcile layout;
   - verify example trees, styles, text updates, and click events.

## Existing strengths

The current arena already provides several useful guarantees:

- generation-checked handles within one arena;
- stable IDs across arena growth, moves, and cloned snapshots;
- detached node construction;
- parent/child validation before mutation;
- cycle prevention;
- one attached window under the app root;
- iterative subtree removal and preorder traversal;
- no-op mutation detection;
- independent per-node revision counters;
- per-node `Arc` copy-on-write storage.

These pieces should be retained while making snapshot ownership explicit and
adding the missing integration layers.

## Current verification status

At the time of this review:

- `cargo test -p burokku --lib` passes all 24 tests;
- Clippy with warnings denied fails because the publication placeholder and two
  test-only helpers are dead code;
- `cargo check --workspace --all-targets` fails because the examples import the
  currently absent `burokku::Burokku` API.

The highest-priority fixes are the incorrect `Div` display mode, the document
bootstrap contract, the shared/item/text style model, and immutable DOM
publication. Those should be resolved before building rendering on top of the
current representation.
