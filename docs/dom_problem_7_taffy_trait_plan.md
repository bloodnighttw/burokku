# Problem 7 implementation plan: Taffy trait-based layout

> Historical implementation plan. Its publication terminology is superseded by [`dom_layout_colocation_plan.md`](dom_layout_colocation_plan.md).

## Purpose

This plan addresses **Problem 7: Taffy reconciliation is missing** in
[`dom_foundation_review.md`](dom_foundation_review.md).

Use Taffy 0.11's low-level, trait-based API instead of constructing a second
mutable `taffy::TaffyTree`. The immutable `DomSnapshot` remains the authoritative
DOM source. Reconciliation lowers it into a revision-scoped, MTS-only
`LayoutTopology` plus sidecars containing converted Taffy styles, algorithm
caches, computed layouts, and measured text state.

## Status

The initial full-rebuild engine is implemented in
`crates/burokku/src/ui/layout/`. It includes the derived topology, stable layout
IDs, style conversion, low-level Taffy traits, complete-output caches, viewport
root constraints, paragraph measurement hooks, baseline propagation, computed
absolute boxes, layout-stage failure-atomic replacement, and focused regression
tests.

The Parley-backed `TextMeasurer`, final-width paragraph resolution, native
Window viewport wiring, revision-tagged Vello scene planning, and WGPU
presentation are now implemented. Positioned stacking contexts and the
incremental `ChangeSet` path remain coordinated with future style work and
Problem 3 respectively. The text-specific work is detailed in
[`dom_problem_6_implementation_plan.md`](dom_problem_6_implementation_plan.md).

## Decision summary

1. Implement Taffy's low-level traits on a short-lived MTS adapter, not on
   `DomSnapshot` itself.
2. Retain one immutable committed snapshot and transactionally lower it into a
   derived `LayoutTopology`. Taffy traversal reads that topology rather than
   calling `DomSnapshot::children()` directly.
3. Keep three distinct relations: the DOM tree for identity/inheritance/events,
   the layout topology for flow and containing blocks, and a later paint tree
   for stacking contexts and final draw order.
4. Use a private `LayoutId` abstraction with a generation-safe mapping from DOM
   `NodeId`. The initial one-box-per-element model may encode the DOM key
   directly, while leaving room for a later explicit arena of synthetic boxes.
5. Omit `App` from layout. The attached `Window` is the root for its native
   logical viewport.
6. Represent ordinary elements as containers and each outer `<text>` element as
   one measured leaf. Nested text elements and raw text nodes receive no layout
   node.
7. Run `compute_root_layout` into scratch topology/sidecars and replace the last
   complete layout only after reconciliation, measurement, validation, and
   final paragraph resolution all succeed.
8. Start with a full lowering for every `ChangeSet::FullRebuild`. Incremental
   batches later update the derived topology and sidecars transactionally.
9. Treat future `z-index` as paint/stacking state, not as a Taffy property.
   `taffy::Layout::order` is not authoritative CSS paint order.
10. Keep logical fractional coordinates. Do not call `round_layout`; the
    renderer applies the logical-to-physical transform once.

## Current-state findings

| Area | Current tree | Planning consequence |
| --- | --- | --- |
| Publication | `PublishedDomReader` atomically supplies `Arc<PublishedDom>` values containing a private, read-only `DomSnapshot` and `ChangeSet::FullRebuild`. | The layout entry point must receive one retained publication. It must never load a second snapshot during the same computation. |
| DOM topology | `DomSnapshot` exposes stable IDs, node kinds, parents, children, and reachable pre-order iteration. Detached arena entries are not returned by `iter()`. | Use it as the authoritative input to a deterministic layout-topology lowering pass, not as the traversal implementation itself. |
| Node identity | Burokku `NodeId` is a `slotmap` key whose FFI representation includes its generation. Taffy 0.11 `NodeId` is a public `u64` wrapper. | A private `LayoutId` can preserve identity across moves and revisions and distinguish a reclaimed slot from its replacement. |
| Styles | Every element style already converts to `taffy::Style<String>` through `Styles::to_taffy_style`. Authoritative grid data uses thread-safe Burokku types and converts on MTS. | Convert once while building the sidecar. Do not implement Taffy's many style traits directly on authoritative DOM structs and do not store `taffy::Style` in a publication. |
| Future positioning | `CommonStyle` does not yet expose CSS-like `position`, inset, or `z-index`. Taffy 0.11 supports only `Relative` and `Absolute`, does not mutate/reparent its input tree, and does not implement stacking contexts. | Make effective layout parentage an explicit derived topology now. Lower future absolute/fixed relationships before Taffy and compute z-index ordering after layout in a separate paint tree. |
| Text | Outer text roots are collected into inherited runs and shaped by the reusable Parley engine. Exact final-width layouts and baselines are retained for paint. | The layout tree supports one paragraph leaf role and an error-aware text callback without inventing raw-text fallback nodes. |
| Taffy | `Cargo.lock` resolves Taffy `0.11.0`; `ui/layout/` implements the low-level traits without a `TaffyTree` mirror. | Retain the characterized interfaces and full-rebuild fallback. |
| Window host | The single committed Window owns a native window and resize/scale events supply its actual logical viewport. | Keep multi-window behavior out of scope until the DOM contract changes. |

## Relevant Taffy 0.11 API

The implementation should use these public low-level interfaces:

| API | Use in Burokku |
| --- | --- |
| `TraversePartialTree` | Return each layout node's effective children from the derived `LayoutTopology`. |
| `LayoutPartialTree` | Return the core style, receive unrounded layouts, and recursively dispatch child layout. |
| `CacheTree` | Store and retrieve per-node `LayoutOutput` values. |
| `LayoutBlockContainer` | Supply block container/item styles and preserve `BlockContext` while recursing. |
| `LayoutFlexboxContainer` | Supply flex container/item styles. |
| `LayoutGridContainer` | Supply grid container/item styles. |
| `compute_block_layout` | Lay out non-empty `Window` and `Div` block containers. |
| `compute_flexbox_layout` | Lay out non-empty `Flex` containers. |
| `compute_grid_layout` | Lay out non-empty `Grid` containers. |
| `compute_leaf_layout` | Apply sizing, padding, margin, and intrinsic measurement to empty boxes and paragraph leaves. |
| `compute_hidden_layout` | Recursively zero a hidden subtree if `Display::None` is introduced. |
| `compute_cached_layout` | Wrap algorithm dispatch with Burokku's `CacheTree` implementation. |
| `compute_root_layout` | Start layout at the committed `Window` root under a definite logical viewport. |

`TraverseTree` may also be implemented because the adapter can recursively
access the complete committed layout tree. It is useful for diagnostics, but
`RoundTree` must not be used in the initial logical-coordinate policy.

A compile spike against Taffy 0.11.0 confirmed that the trait implementation,
`BlockContext` forwarding, and direct `slotmap` key round-trip compile without
the high-level `taffy_tree` feature.

## Why not `TaffyTree`

A derived layout topology is necessary once layout parentage can differ from DOM
parentage. `TaffyTree` does not remove that need: Taffy 0.11 does not discover
CSS containing blocks, reparent nodes, implement `fixed`/`sticky`, or construct
z-index stacking contexts for Burokku.

Using `TaffyTree` would therefore produce both a Burokku semantic lowering and a
second independently mutable generic tree whose insertions, removals, moves,
reorders, and skipped revisions must be replayed correctly. Instead, build one
transactional, revision-scoped representation designed for Burokku:

```text
Arc<PublishedDom>
└── DomSnapshot                  authoritative DOM identity and relationships

MTS derived state
├── LayoutTopology              effective layout parents and ordered children
├── LayoutNodeState             converted style, cache, layout, paragraph role
└── future PaintTree            stacking contexts and final paint order
```

`LayoutTopology` is not authoritative application state. It is a deterministic
lowering of one retained publication and may be discarded wholesale after a
failed frame. Initially it mirrors DOM parentage except for App omission and
text flattening. Future position support can change effective layout edges
without mutating the DOM or replacing the Taffy trait adapter.

Do not implement the Taffy traits directly on `DomSnapshot`. The traits contain
mutation methods such as `set_unrounded_layout`, and forcing those through the
snapshot would either violate publication immutability or introduce interior
mutability at the wrong ownership boundary.

## Ownership and frame boundary

```text
BTS staging Dom
    -> runtime checkpoint
    -> Arc<PublishedDom> stored atomically
    -> MTS loads one Arc
    -> lower snapshot into scratch LayoutTopology + node sidecars
    -> trait adapter borrows topology + scratch sidecars + TextEngine
    -> Taffy compute_root_layout
    -> final paragraph resolution
    -> finite/layout invariant validation
    -> atomically replace MTS ComputedLayout
    -> build paint/stacking tree from the same revision
    -> Vello scene construction and presentation
```

The topology, adapter, node sidecars, and future paint tree are MTS-only. They do
not need `Arc<Mutex<_>>` or other cross-thread synchronization. The publication
is the only shared DOM boundary. Each arrow is revision-tagged, but the latest
publication, current `ComputedLayout`, and successfully presented scene may
carry different revisions temporarily as rendering lags or skips intermediate
publications.

## Three distinct relations

Do not overload one parent relation with three different semantics:

```text
DOM tree
  identity, attachment, text/style inheritance, events, bubbling

LayoutTopology
  generated boxes, normal flow, containing blocks, Taffy traversal

PaintTree / StackingContextTree
  clips, stacking-context ownership, z-index groups, final draw order
```

A positioned element keeps its DOM parent for JavaScript behavior and event
bubbling even if its effective layout parent becomes an earlier containing
block. Likewise, paint order may differ from both DOM sibling order and layout
child order without changing either tree.

## Stable layout-node mapping

### Initial `LayoutId`

Every initial layout box corresponds to exactly one Burokku element node. Hide
Taffy's raw ID behind a private `LayoutId` abstraction, while initially deriving
its value from the complete generation-checked DOM key:

```rust
use slotmap::{Key, KeyData};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct LayoutId(taffy::NodeId);

fn layout_id_for_dom(id: DomNodeId) -> LayoutId {
    LayoutId(taffy::NodeId::new(id.data().as_ffi()))
}

fn dom_id_for_layout(id: LayoutId) -> DomNodeId {
    DomNodeId::from(KeyData::from_ffi(u64::from(id.0)))
}
```

Keep these helpers private and validate all accesses through the current
`LayoutTopology` maps before indexing. Public layout, paint, hit-test, and event
APIs remain keyed by Burokku `NodeId`, never by `LayoutId` or
`taffy::NodeId`.

This initial mapping provides:

- the same layout ID after DOM reparenting, effective layout reparenting, or
  sibling reordering;
- the same ID for the same node in later snapshots;
- a different ID when `slotmap` reuses a slot with a newer generation;
- deterministic IDs during full rebuilding.

Add a pinned-version round-trip test, including a removed-and-reallocated slot.
If anonymous layout boxes, static-position anchors, generated wrappers, or
multiple fragments per DOM node become necessary, retain the `LayoutId` API but
replace its direct encoding with an explicit layout-ID arena and bidirectional
maps. Do not reserve unchecked bits in the opaque `slotmap` representation.

### Nodes that receive an ID

| DOM node | Layout representation |
| --- | --- |
| `NodeKind::App` | No layout node. |
| Attached `Element::Window` | One block root constrained to its native logical viewport. |
| Attached `Element::Div` | One block box. |
| Attached `Element::Flex` | One flex box. |
| Attached `Element::Grid` | One grid box. |
| Outermost `Element::Text` | One measured paragraph leaf. |
| Nested `Element::Text` | No independent box; recorded as consumed by its paragraph source. |
| Raw `NodeKind::Text` | No independent box; recorded as consumed by its paragraph source. |
| Detached element or text node | No sidecar and no active layout node. |

Changing an element's effective layout parent does not change its `LayoutId`.
"Absent" for cleanup means absent from the reachable app tree, not merely absent
from the snapshot arena. A detached node can still exist for a live JavaScript
wrapper while correctly having no layout state.

## MTS derived-state model

A suitable initial shape is:

```rust
struct LayoutNodeState {
    dom_id: DomNodeId,
    role: LayoutRole,
    style: taffy::Style<String>,
    revisions: NodeRevisions,
    cache: NodeLayoutCache,
    unrounded: taffy::Layout,
}

enum LayoutRole {
    Container,
    Paragraph {
        input: Rc<ParagraphInput>,
    },
}

struct LayoutTopology {
    root: Option<LayoutId>,
    parent: HashMap<LayoutId, LayoutId>,
    children: HashMap<LayoutId, Vec<LayoutId>>,
    dom_to_layout: HashMap<DomNodeId, LayoutId>,
    layout_to_dom: HashMap<LayoutId, DomNodeId>,
    positioning: HashMap<LayoutId, PositioningMeta>,
}

struct PositioningMeta {
    dom_parent: Option<DomNodeId>,
    containing_block: Option<LayoutId>,
    source_order: u32,
}

struct ScratchLayout {
    revision: u64,
    viewport: LogicalViewport,
    window: Option<DomNodeId>,
    topology: LayoutTopology,
    nodes: HashMap<LayoutId, LayoutNodeState>,
    text_owner: HashMap<DomNodeId, DomNodeId>,
    final_paragraphs: HashMap<DomNodeId, Rc<ShapedParagraph>>,
}

struct ComputedLayout {
    publication: Arc<PublishedDom>,
    revision: u64,
    viewport: LogicalViewport,
    window: Option<DomNodeId>,
    topology: LayoutTopology, // private derived state
    boxes: HashMap<DomNodeId, ComputedBox>,
    final_paragraphs: HashMap<DomNodeId, Rc<ShapedParagraph>>,
    text_owner: HashMap<DomNodeId, DomNodeId>,
}
```

The exact public/private split may differ, but preserve these rules:

- converted Taffy styles and effective layout edges are MTS-only derived values;
- internal topology and node sidecars are keyed by `LayoutId`;
- renderer-, hit-test-, and event-facing state remains keyed by DOM `NodeId`;
- paragraph input is owned and does not borrow the publication;
- renderer-facing state is read-only and revision tagged;
- the last complete `ComputedLayout` remains unchanged while layout scratch
  state is computed; later scene or presentation state is a separate stage.

### Topology is lowered from the snapshot

The initial lowering deliberately mirrors the DOM for ordinary layout elements:

- omit App and make Window the layout root;
- preserve DOM parentage and sibling order for `Window`, `Div`, `Flex`, `Grid`,
  and outer text boxes;
- expose no layout children for paragraph leaves;
- flatten nested text elements and raw text into their paragraph owner;
- omit detached nodes.

`TraversePartialTree` reads only `LayoutTopology::children`. It never decides
positioning semantics and never calls `DomSnapshot::children()` during Taffy
computation. The reconciler is the sole place that converts DOM relationships
into effective layout relationships.

When position support arrives, the lowering can change an element's effective
layout parent while preserving `PositioningMeta::dom_parent` and source order.
Preflight validation must prove that the derived topology has one root, no
cycles, matching forward/reverse mappings, exactly one parent per non-root box,
and a sidecar for every exposed ID.

## Full reconciliation and lowering algorithm

For every publication carrying `ChangeSet::FullRebuild`:

1. Validate the publication's target revision against its snapshot.
2. Validate that `App` has zero children or one attached `Window` child.
3. If no Window is attached, build a successful empty layout outcome. This is
   not a Taffy error and lets Problem 10 remove native resources cleanly.
4. Traverse the reachable DOM iteratively from the Window.
5. Track visited DOM IDs and depth so corrupted or adversarial input cannot
   enter Taffy's recursive algorithms unchecked.
6. For each `Window`, `Div`, `Flex`, or `Grid`:
   - allocate/derive its stable `LayoutId`;
   - convert the authoritative style once;
   - create a fresh `LayoutNodeState`;
   - record its DOM parent and source order.
7. For an outer text element:
   - create one paragraph layout ID and sidecar using the outer box style;
   - collect one owned `ParagraphInput`;
   - record every nested text/raw descendant in `text_owner`;
   - create no layout IDs for those consumed descendants.
8. Treat a nested text element reached as an ordinary box, or a raw text node
   reached outside paragraph collection, as a typed DOM invariant error.
9. Ignore detached arena entries because they are not reachable from App.
10. Lower the classified boxes into effective parent/child lists. Initially this
    mirrors DOM layout-box parentage and source order. Future position support
    substitutes containing-block edges in this step.
11. Validate topology mappings, parent uniqueness, root, acyclicity, child
    order, sidecar membership, and paragraph leaf status.
12. Run Taffy only after the complete scratch representation validates.
13. Build absolute boxes and resolve the exact final paragraphs by traversing
    the same derived topology.
14. Prune persistent text-cache sources and publish the new computed state only
    after the complete layout succeeds.

A full rebuild creates fresh algorithm caches. That still caches repeated probes
within one Taffy run, which is where flex/grid and text measurement need it most.
The same revision and same viewport should reuse the already computed frame
without rebuilding.

## Trait adapter

Use a temporary adapter similar to:

```rust
struct DerivedLayoutTree<'a> {
    topology: &'a LayoutTopology,
    nodes: &'a mut HashMap<LayoutId, LayoutNodeState>,
    text: &'a mut TextEngine,
    first_error: Option<LayoutError>,
}
```

The adapter does not need the DOM snapshot for traversal. Reconciliation has
already converted the retained publication into validated effective layout
relationships and owned node inputs.

### Trait responsibilities

| Trait method | Behavior |
| --- | --- |
| `child_ids`, `child_count`, `get_child_id` | Read effective ordered children from `LayoutTopology`; paragraph leaves already have empty child lists. |
| `get_core_container_style` | Borrow the converted `taffy::Style<String>` in the sidecar. |
| `set_unrounded_layout` | Copy the result into the scratch sidecar. |
| `cache_get`, `cache_store`, `cache_clear` | Delegate to the bounded, baseline-preserving per-node cache. Do not store after an error is latched. |
| block/flex/grid style getters | Return the same converted style reference; `taffy::Style<String>` implements all required style traits. |
| `compute_child_layout` | Delegate to one internal dispatch function with no block context. |
| `compute_block_child_layout` | Delegate to the same internal dispatch function while forwarding Taffy's `BlockContext`. |

Forwarding `BlockContext` is important. The default
`LayoutBlockContainer::compute_block_child_layout` discards it; mirror Taffy's
own tree adapter so nested block layout retains the context expected by the
pinned algorithm.

### Algorithm dispatch

Conceptually:

```rust
fn compute_node_layout(
    &mut self,
    node: taffy::NodeId,
    input: LayoutInput,
    block_context: Option<&mut BlockContext<'_>>,
) -> LayoutOutput {
    if input.run_mode == RunMode::PerformHiddenLayout {
        return compute_hidden_layout(self, node);
    }

    compute_cached_layout(self, node, input, |tree, node, input| {
        match tree.prevalidated_class(node) {
            Hidden => compute_hidden_layout(tree, node),
            NonEmptyBlock => {
                compute_block_layout(tree, node, input, block_context)
            }
            NonEmptyFlex => compute_flexbox_layout(tree, node, input),
            NonEmptyGrid => compute_grid_layout(tree, node, input),
            EmptyBox => compute_leaf_layout(
                input,
                tree.style(node),
                resolve_calc,
                zero_measure,
            ),
            Paragraph => tree.compute_paragraph_leaf(node, input),
        }
    })
}
```

Taffy's trait returns `LayoutOutput`, not `Result`. All structural
classification should therefore be validated before layout, and runtime
measurement failures use the error-latching policy below.

Dispatch based on the sidecar role plus converted `Display`; do not infer a
paragraph merely because an ordinary box has no children. Empty `Div`, `Flex`,
or `Grid` nodes use `compute_leaf_layout` with zero intrinsic content while
retaining their box sizing and display style. Position lowering is complete
before dispatch, so these methods never mutate or reinterpret topology.

## Cache policy

Do not initially use `taffy::Cache` unchanged for the trait adapter. In Taffy
0.11.0, a `RunMode::ComputeSize` cache entry stores only `Size<f32>` and a cache
hit reconstructs `LayoutOutput::from_outer_size`. That drops
`first_baselines`, which would make text baseline alignment depend on cache-hit
order.

Implement a small Burokku `NodeLayoutCache` that:

- stores the complete `LayoutOutput`, including baseline and content size;
- keys entries by the complete `LayoutInput` rather than a partial ad hoc key;
- uses exact finite input equality for correctness, accepting extra misses over
  Taffy's approximate cache matching;
- keeps one final-layout entry and a bounded set of measurement entries (start
  with at most 16);
- evicts or clears safely when full; a miss may cost time but cannot alter
  layout;
- clears on full rebuild and on every later dirty-propagation path;
- ignores reads and writes after `first_error` is set.

All definite viewport and style inputs must be validated as finite before
Taffy runs. A non-finite generated input should latch an internal layout error
instead of becoming a cache key.

Characterize both cached and uncached baseline-aligned flex/grid fixtures so a
future Taffy upgrade cannot silently reintroduce baseline loss.

## Text leaves and baseline support

The low-level API should replace the high-level
`TaffyTree::compute_layout_with_measure` callback assumed by the earlier text
plan.

For a paragraph leaf:

1. Call `compute_leaf_layout` with the outer text element's converted box style.
2. In its measure closure, map `AvailableSpace::{MinContent, MaxContent,
   Definite}` to the text cache constraints from the Problem 6 plan.
3. Return Parley's content width and height only. Taffy remains responsible for
   margin, padding, border, explicit dimensions, and final position.
4. After `compute_leaf_layout` returns an outer size, obtain the matching Parley
   variant for that output's content-box width, including the case where Taffy
   skipped the measure closure because dimensions were already known.
5. Set `LayoutOutput::first_baselines.y` to the paragraph's first baseline plus
   the resolved top border and padding. Taffy expects a baseline relative to the
   outer box, while Parley's baseline is relative to the paragraph content
   origin.
6. Preserve that complete output in `NodeLayoutCache`.

This removes the previously documented "no typographic baseline" limitation
for measured paragraph leaves: the trait API can return
`LayoutOutput::first_baselines`, while the high-level measure callback can
return only `Size<f32>`. Baseline propagation through non-text containers
remains whatever Taffy 0.11's selected block/flex/grid algorithms provide; do
not claim browser inline-baseline behavior for an intervening `Div`.

Taffy may request several constraints and may later choose a different final
width. After root layout, repeat the Problem 6 final paragraph-resolution pass:

- read each paragraph leaf's final content-box width;
- retrieve or build the exact finite-width Parley variant;
- retain it in `final_paragraphs` for this revision;
- make painting consume only that final paragraph.

Nested text nodes remain absent from Taffy regardless of which text variants are
measured.

## Error propagation and atomic replacement

Taffy's low-level compute traits are infallible at the type level. Burokku text
collection, shaping, font lookup, and finite-value checks are fallible.

Use this policy:

1. Perform every structural and style validation possible before invoking
   Taffy.
2. Keep `first_error: Option<LayoutError>` on the temporary adapter.
3. If a measure operation fails, store only the first error and return a
   deterministic zero content size so Taffy can unwind normally.
4. Once an error is latched, skip further expensive measurement and do not read
   or write algorithm caches.
5. After `compute_root_layout` returns, propagate the latched error and discard
   all scratch layout state.
6. Validate every committed layout size, location, inset, and final paragraph
   metric as finite before replacement.
7. A layout-stage failure leaves the previous `ComputedLayout` current. If a
   later scene stage fails after layout succeeded, the newer computed layout may
   remain current while the last successfully presented scene/hit-test plan
   remains active. Neither case rolls back the latest DOM publication.

Do not panic or use `catch_unwind` for expected shaping or invalid-input errors.
Impossible IDs inside a prevalidated adapter are internal bugs and should carry
clear debug assertions.

## Native Window viewport contract

Adopt one explicit contract with Problem 10:

- `WindowStyle.width` and `height` describe the requested native window size at
  creation or an explicit native resize request;
- after realization, the actual native content viewport in logical pixels is
  authoritative for root layout;
- the Taffy style stored for the Window root therefore uses the actual logical
  viewport as its exact width and height;
- do not apply the requested Window dimensions a second time as a smaller CSS
  root inside the already-created native viewport.

The layout API should accept:

```rust
struct LogicalViewport {
    width: f32,
    height: f32,
}
```

Both values must be finite and non-negative. Build a Window-root Taffy style
whose size is exactly that viewport, then call `compute_root_layout` with
matching `AvailableSpace::Definite` values. Root location is `(0, 0)`.

A native resize invalidates layout even when the DOM revision did not change.
Key reuse therefore requires both `(revision, viewport)`. Display scale is not
part of this key under the initial logical-coordinate and unquantized text
policy.

Same-ID Window updates are best-effort rather than strictly transactional:
validate the complete requested spec first, issue the fallible size request
before changing the title, and update the stored spec only after those calls
succeed. A platform-accepted resize is not rolled back; the resulting actual
native viewport remains authoritative. For a true replacement, prepare the
candidate renderer before destroying the old Window. Removing the final Window
remains an immediate committed lifecycle operation.

If the product instead wants a fixed-size canvas inside a resizable native
window, define that as a separate element/style behavior. Do not leave Window
size semantics dependent on whether Taffy happens to treat an `auto` block root
as stretched on one axis.

## Final computed boxes

Taffy layouts are relative to the effective layout parent, which may differ
from the DOM parent. After a successful root computation, walk
`LayoutTopology` iteratively and derive renderer-facing data:

```rust
struct ComputedBox {
    layout: taffy::Layout,
    layout_parent: Option<DomNodeId>,
    border_origin: Point<f32>,
    content_origin: Point<f32>,
}
```

For each effective child, add its relative `layout.location` to the effective
parent's absolute border origin. Derive the content origin from border plus
padding exactly once. Store only finite results.

This post-pass gives rendering and later hit testing one immutable geometry map
keyed by DOM `NodeId`; neither consumer needs the mutable trait adapter or a
Taffy ID. Preserve `taffy::Layout::order` for diagnostics and algorithm-local
source order, but do not treat it as final paint order once stacking contexts or
`z-index` exist.

## Future positioning and effective layout parents

Taffy 0.11 does not mutate or reparent the tree supplied through
`TraversePartialTree`. It supports only `Position::Relative` and
`Position::Absolute`; it has no native `Static`, `Fixed`, or `Sticky` variant.
Burokku must therefore lower its future positioning contract explicitly.

When `CommonStyle` gains position and inset values, use these initial rules:

- **Static:** remain under the normal-flow layout parent. Lower to Taffy's
  relative mode with zero relative inset, but do not register the node as a
  containing block merely because Taffy lacks a static enum.
- **Relative:** remain under the normal-flow parent, participate in flow, apply
  Taffy's relative inset, and register as a containing block according to the
  Burokku contract.
- **Absolute:** remove the box from its DOM parent's normal-flow child list and
  attach it to the nearest eligible containing block in `LayoutTopology`; expose
  it to Taffy as an absolute direct child of that effective parent.
- **Fixed:** attach to the Window viewport/root containing block and lower as an
  absolute box under that root.
- **Sticky:** keep normal-flow parentage and apply a scroll-dependent adjustment
  after base layout. Do not claim sticky support by mapping it to relative.

The DOM parent remains unchanged for inheritance, wrappers, connectedness, and
event bubbling. Preserve at least the original DOM parent, source sibling order,
resolved containing block, and any required static-position anchor in
`PositioningMeta`.

Reparenting an absolute box to its containing block can change the static
position that Taffy would infer from direct-child order. Before claiming full
CSS static-position behavior, characterize the required cases. If source
metadata is insufficient, introduce an explicit synthetic anchor `LayoutId` or
a two-pass static-position calculation; do not silently approximate it.

Maintain a reverse dependency index from containing blocks to positioned
descendants. Changing a node between static/relative positioning may change the
containing block of multiple descendants even when those descendants' own
styles did not change.

## Future stacking contexts and paint order

`z-index` is not a Taffy layout property. Taffy 0.11 does not build CSS stacking
contexts, and `taffy::Layout::order` is only algorithm/source ordering. Problem
8 should build a separate paint representation after successful layout:

```rust
struct PaintTree {
    root: StackingContextId,
    contexts: Vec<StackingContext>,
    paint_order: Vec<PaintItem>,
}

struct PaintItem {
    node: DomNodeId,
    box_id: LayoutId,
    phase: PaintPhase,
}
```

The stacking builder consumes the same retained publication, computed boxes,
and positioning metadata. It must define, at minimum:

- which styles create a stacking context;
- `z-index: auto` versus signed integer values;
- negative contexts, parent background/border, in-flow content, positioned
  auto/zero content, and positive contexts;
- stable DOM/source-order tie breaking;
- nested stacking contexts as atomic paint units;
- clip inheritance and future transform/opacity context creation.

Do not implement z-index as one global integer sort: that breaks nested stacking
contexts. Scene construction follows `paint_order`, and pointer hit testing
walks the successfully presented order in reverse. The geometry and stacking
data used to build one candidate scene must match that scene's revision and
viewport; once presented, its hit-test order remains paired with its pixels even
if a newer publication or computed layout is already pending.

A z-index-only change is paint/stacking dirty and normally must not clear Taffy
geometry caches. A position or inset change is layout-topology dirty and may
also be paint-order dirty.

## Incremental reconciliation follow-up

The first implementation consumes only `ChangeSet::FullRebuild`. When Problem 3
adds a bounded incremental variant, apply it transactionally to a scratch copy
of the previous sidecars.

### Eligibility

Use an incremental batch only when:

- its source revision equals the current computed revision;
- its target revision equals the retained publication revision;
- the current viewport state is compatible;
- every referenced ID is valid for the appropriate source or target state.

Otherwise perform a full rebuild. MTS is allowed to skip publications, so this
fallback is part of the normal path rather than an exceptional condition.

### Derived-state updates

- **Inserted:** create a layout mapping, converted style, normal/effective edge,
  positioning metadata, and fresh empty cache.
- **Removed/detached:** determine dirty old effective ancestors, then remove all
  unreachable topology edges, sidecars, dependency entries, and text owners.
- **DOM moved/reordered:** update normal-flow source edges/order, rerun effective
  parent lowering for affected boxes, and clear old/new effective parent chains.
- **Position/inset changed:** recompute containing blocks and effective edges for
  the node plus positioned descendants that depended on its old/new containing
  block status; clear every affected layout ancestor.
- **Layout style changed:** reconvert the style and clear that node plus every
  effective layout ancestor.
- **Text changed:** recollect the enclosing paragraph, replace its input, and
  clear the paragraph plus effective ancestors.
- **Viewport changed:** replace the root style and clear from the Window root;
  fixed-position descendants remain attached to that root.
- **Z-index/stacking changed:** rebuild affected stacking contexts and paint/hit
  order without clearing Taffy geometry when no layout property changed.
- **Other paint-only changed:** retain Taffy state once revisions are split
  finely enough to prove the change is paint-only.

The future change batch must retain enough old DOM parent, old effective parent,
old containing-block, removal, and source-order information to invalidate state
that no longer exists in the target topology. If it does not, fall back to a
full rebuild.

Commit the incrementally reconciled state only after layout and final paragraph
resolution succeed, just like the full path.

## Dependency configuration

Once the adapter compiles in the crate, stop enabling Taffy's high-level tree
and unrelated default algorithms implicitly. The intended initial feature set
is:

```toml
taffy = {
    version = "0.11",
    default-features = false,
    features = [
        "std",
        "block_layout",
        "flexbox",
        "grid",
        "content_size",
        "parse",
    ],
}
```

Keep the lockfile on the characterized 0.11 release. `taffy_tree`,
`float_layout`, `detailed_layout_info`, and `calc` are unnecessary for the
current style contract. Enable one later only with the corresponding Burokku
style/API and tests.

Run the complete crate tests before landing this feature change because
Burokku's authoritative style modules use several Taffy value enums even though
they do not use `TaffyTree`.

## Proposed module layout

```text
crates/burokku/src/ui/
├── layout.rs
├── layout/
│   ├── engine.rs       # revision/viewport entry point and atomic replacement
│   ├── reconcile.rs    # snapshot classification and transactional lowering
│   ├── topology.rs     # LayoutId maps, effective parents, ordered children
│   ├── tree.rs         # Taffy trait adapter and algorithm dispatch
│   ├── cache.rs        # complete LayoutInput -> LayoutOutput cache
│   ├── computed.rs     # immutable revision-tagged geometry output
│   └── error.rs        # viewport, invariant, text, and finite-value failures
├── paint.rs            # Problem 8 scene/paint boundary
├── paint/
│   └── stacking.rs     # future stacking contexts and final paint order
└── text/               # paragraph collection, Parley engine/cache, resolution
```

Suggested file-level changes:

| File/path | Planned change |
| --- | --- |
| `crates/burokku/Cargo.toml` | Use the explicit low-level Taffy feature set; add Problem 6 text dependencies when that stage begins. |
| `crates/burokku/src/ui.rs` | Register the crate-private layout and text modules. |
| `crates/burokku/src/ui/elements.rs` | Optionally add one crate-private `Element::to_taffy_style()` dispatcher so reconciliation does not duplicate variant matching. Do not add computed state to DOM nodes. |
| `crates/burokku/src/ui/elements/styles/common.rs` | Later add the explicit Burokku position/inset/z-index contract; keep layout-affecting and stacking-only invalidation distinguishable. |
| `crates/burokku/src/ui/layout*` | Add `LayoutId`, derived topology, sidecars, full lowering, trait implementations, layout entry point, errors, and computed boxes. |
| `crates/burokku/src/ui/elements/publication.rs` | No first-pass protocol change. Continue consuming `FullRebuild`; extend only when a real incremental batch exists. |
| `crates/burokku/src/ui/text*` | Supply paragraph inputs, fallible measurement, baselines, and final-width resolved layouts. |
| future Window host | Supply the actual logical viewport and coalesce commit/resize redraw requests. |
| future renderer/paint modules | Build a revision-tagged stacking-context tree and paint order from one `ComputedLayout`; the presented result may lag later computed state. |

## Implementation stages

### Stage 0: pin and characterize the low-level API

- Switch a private compile fixture to the explicit Taffy feature set.
- Implement the minimal trait set over a small test tree.
- Verify DOM/`LayoutId`/Taffy ID round trips across slot generation reuse.
- Verify `BlockContext` forwarding compiles.
- Characterize `compute_leaf_layout` padding/known-dimension inputs.
- Characterize first-baseline behavior and the baseline loss in
  `taffy::Cache` measurement entries.
- Confirm that fractional layouts remain unchanged when `round_layout` is not
  called.
- Characterize Taffy's recursive depth on supported MTS stacks and select a
  conservative maximum layout depth before processing script-created trees.

**Exit criteria:** the exact Taffy 0.11 interfaces and assumptions used below
are covered by compiling tests, including a documented safe-depth policy.

### Stage 1: full snapshot classification, lowering, and sidecars

- Add `LogicalViewport`, `LayoutId`, `LayoutTopology`, layout roles, sidecars,
  errors, and ID conversion.
- Traverse only the committed reachable DOM tree.
- Omit App and consumed text descendants.
- Build initial effective edges that mirror normal DOM box parentage/order.
- Convert all ordinary element styles.
- Initially allow a test paragraph measurer before Parley lands.
- Validate root, mappings, parent uniqueness, acyclicity, child order, and
  sidecar membership.
- Add successful no-Window handling and enforce the selected maximum depth.

**Exit criteria:** fixtures produce the expected topology, sidecar ID set, role
set, style values, child order, and owner map without constructing
`TaffyTree`.

### Stage 2: trait adapter and block/flex/grid layout

- Implement `TraversePartialTree`, `LayoutPartialTree`, `CacheTree`, and the
  three container traits over `LayoutTopology`.
- Prove Taffy traversal does not consult or reinterpret DOM child edges.
- Add complete-output caches and unified algorithm dispatch.
- Run from Window using a validated definite viewport.
- Derive immutable absolute `ComputedBox` values through effective parent edges.
- Commit only successful scratch state.

**Exit criteria:** block, flex, and grid fixtures match expected sizes and
positions; moves, reorderings, removals, and viewport changes are reflected by a
full rebuild.

### Stage 3: Parley measured leaves and baselines

- Integrate the Problem 6 collector and `TextEngine`.
- Measure min-content, max-content, and definite widths through
  `compute_leaf_layout`.
- Return the first typographic baseline in `LayoutOutput`.
- Resolve exact final-width layouts after Taffy.
- Keep nested spans out of Taffy and retain owner mappings.

**Exit criteria:** one outer paragraph creates one measured leaf, flex/grid
baseline behavior is deterministic on cache hits and misses, and painting can
consume the exact final Parley object.

### Stage 4: Window host and frame integration

- Pass actual logical native viewport dimensions into the engine.
- Observe and retain one latest `Arc<PublishedDom>` as the content target,
  independently from best-effort native WindowSpec application.
- Recompute on revision or viewport changes and reuse on exact matches.
- Coalesce notifier and resize redraw requests without immediately retrying an
  unchanged failed candidate.
- Keep layout-stage replacement atomic, but allow computed state to advance
  beyond the last successfully presented scene.
- Preserve the last presented scene/hit-test plan after a recoverable candidate
  failure while leaving the latest DOM publication authoritative.

**Exit criteria:** each layout/scene candidate uses one tagged publication and
actual viewport, stage revisions may differ through normal pipeline lag, and no
layout work holds the BTS DOM mutex.

### Stage 5: future positioned topology and stacking boundary

- Add the chosen Burokku `position`, inset, and `z-index` style contract.
- Resolve containing blocks and lower absolute/fixed effective edges before
  Taffy; keep relative/static boxes in normal flow.
- Preserve DOM parent/source order and characterize absolute static-position
  behavior, adding synthetic anchors only if required.
- Add the containing-block dependency index.
- Build the Problem 8 stacking-context tree and paint order after layout.
- Make hit testing use reverse presented paint order.
- Prove z-index-only changes do not alter Taffy geometry.

**Exit criteria:** positioned geometry, paint order, and hit order match the
specified Burokku subset without changing JavaScript-visible DOM relationships.

### Stage 6: incremental changes and performance

- Add the bounded incremental `ChangeSet` in the publication workstream.
- Apply topology/sidecar updates and effective-ancestor cache invalidation
  transactionally.
- Rebuild stacking contexts independently for stacking-only changes.
- Fall back on skipped/malformed batches.
- Benchmark full classification, topology lowering, style conversion, Taffy
  compute, cache hit rates, paragraph probes, and absolute-box extraction.
- Re-evaluate the documented depth and resource limits from measurements,
  without weakening the pre-Taffy safety check.

**Exit criteria:** incremental and forced-full geometry/topology results are
structurally and numerically equivalent for the same publication and viewport;
stacking-only updates preserve those results.

## Required tests

### Identity and reconciliation

- DOM/`LayoutId`/Taffy round-trip preserves index and generation.
- Reclaimed/reallocated DOM slots map to different layout IDs.
- App has no topology entry; Window is the root.
- Detached nodes have no sidecar or topology entry even while present in the
  snapshot arena.
- DOM moves and future effective reparenting preserve layout IDs.
- Initial normal-flow child order exactly follows the committed snapshot.
- Forward/reverse maps, parent/children maps, and sidecar membership agree.
- Removed subtrees and detached former children disappear on full rebuild.
- A retained old publication and computed frame remain valid after a newer
  publication is reconciled.

### Structure and styles

- The initial lowering mirrors DOM box parents/order while omitting App and
  consumed text descendants.
- Div, Flex, and Grid select block, flex, and grid algorithms respectively.
- Empty ordinary elements use zero intrinsic leaf measurement without becoming
  paragraphs.
- Container and item properties survive style conversion.
- A raw text node outside a text element is rejected defensively.
- An outer text tree creates one layout node regardless of nested span depth.
- Cyclic, multiply parented, unmapped, or sidecar-less derived nodes fail before
  Taffy runs.
- Deep input is rejected at the chosen safe depth before Taffy recursion.

### Cache and failure behavior

- Equal complete inputs reuse complete `LayoutOutput` values.
- Different parent sizes, available spaces, axes, sizing modes, or run modes do
  not collide.
- Measurement cache hits preserve `first_baselines`.
- Cache eviction changes performance only, not numerical results.
- Dirty propagation clears all required ancestors in incremental tests.
- A measurement failure writes no reusable algorithm result and leaves the
  previous computed frame current.
- NaN, infinity, negative viewport sizes, or non-finite outputs never become a
  committed frame.

### Viewport and coordinates

- Window root size equals the supplied logical viewport.
- A resize recomputes the same DOM revision under the new viewport.
- Fractional logical positions remain fractional.
- Absolute border and content origins add parent offsets, border, and padding
  exactly once.
- A publication arriving during layout is deferred to a later frame.

### Text integration

- Min-content, max-content, and finite constraints reach the expected Parley
  variants.
- Explicit text dimensions still produce a final paint paragraph if Taffy skips
  intrinsic measurement.
- Padding contributes exactly once to text size and origin.
- Parley's first baseline includes the outer box's top inset exactly once.
- Baseline-aligned flex/grid output is identical with warm and cold caches.
- Nested text style/content changes invalidate the enclosing paragraph on the
  next full rebuild.

### Future positioning and paint order

- Static/relative nodes keep normal-flow effective parents.
- Absolute nodes attach to the selected containing block without changing their
  DOM parent or event path.
- Fixed nodes use the Window viewport; sticky behavior remains explicitly
  unsupported until its post-layout adjustment exists.
- Changing containing-block status re-lowers dependent positioned descendants.
- DOM source metadata preserves or explicitly rejects unsupported absolute
  static-position cases.
- Negative, auto/zero, and positive z-index groups obey nested stacking-context
  boundaries and stable source-order ties.
- A z-index-only change preserves computed Taffy geometry and final paragraphs.
- Hit testing in reverse presented paint order selects the topmost painted node.

## Validation commands

```bash
cargo fmt --check
cargo test -p burokku --lib
cargo test -p burokku layout
cargo test -p burokku text
cargo clippy -p burokku --all-targets --all-features -- -D warnings
cargo test --workspace
```

Add a forced-cache-off test mode for deterministic comparison with cached
results, and add focused Criterion benchmarks only after the correctness path is
stable.

## Completion criteria

Problem 7 is complete for the initial full-rebuild contract when:

- layout consumes one retained immutable `PublishedDom` and explicit logical
  viewport;
- reconciliation transactionally lowers that publication into a validated,
  revision-scoped `LayoutTopology`;
- Taffy's low-level traits traverse only effective topology edges, without a
  mutable `TaffyTree` mirror or direct dependence on DOM child traversal;
- every reachable layout element has a stable generation-safe mapping and no
  detached or consumed text node has an active layout sidecar;
- block, flex, grid, empty boxes, and measured paragraph leaves dispatch to the
  correct algorithms;
- native viewport changes constrain the Window root and invalidate layout;
- full rebuilds correctly handle creation, removal, DOM reparenting, order,
  style, and text changes;
- text measurement returns complete dimensions and typographic baselines without
  cache-dependent results;
- final computed boxes and final paragraphs all carry the publication revision
  and viewport used to produce them;
- a layout-stage failure leaves the previous complete `ComputedLayout` intact,
  while a later scene failure may leave newer computed state alongside the last
  successfully presented scene/hit-test plan; the latest DOM is never rolled
  back;
- the topology abstraction can later represent containing-block reparenting
  without changing DOM identity or the Taffy trait surface;
- future paint ordering is explicitly owned by a stacking-context tree rather
  than `taffy::Layout::order`;
- the design has a defined fallback from future incremental batches to the same
  full-lowering result.
