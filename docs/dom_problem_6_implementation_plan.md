# Problem 6 implementation plan: text collection, layout, and painting

> Historical implementation plan. Its publication terminology is superseded by [`dom_layout_colocation_plan.md`](dom_layout_colocation_plan.md).

## Purpose

This is an execution plan for problem 6 in
[`dom_foundation_review.md`](dom_foundation_review.md): complete the native text
pipeline from an immutable committed DOM revision through paragraph collection,
Parley shaping, Taffy measurement, and Vello Hybrid glyph submission.

The behavioral contract remains
[`text_rendering_plan.md`](text_rendering_plan.md). This document records what is
already implemented in the current tree, resolves the integration details that
are specific to the current dependency versions, and divides the remaining work
into mergeable stages.

## Status

Stages 0 through 6 are implemented for the initial full-rebuild pipeline:
dependency compatibility, styled-run collection, Parley shaping/cache, Taffy
measurement, exact final-width paragraph resolution, Vello glyph submission,
and native scene presentation. Stage 7 and incremental text-dirty batches remain
follow-up work.

## Current-state findings

| Area | Current implementation | Planning consequence |
| --- | --- | --- |
| Raw and styled text | `NodeKind::Text(String)` and `Element::Text` are distinct. `NodeKind::accepts` now permits attached raw text only beneath text elements, while text elements may nest recursively. | Preserve this strict child matrix; do not add anonymous or standalone raw-text layout nodes. |
| DOM text APIs | `Dom::text_content`, `Dom::set_text_content`, and `Dom::set_text` are implemented and tested. Raw/text-node updates work, while `App` and non-text elements reject `textContent` assignment without mutation. | The DOM semantics needed by collection are complete; retain their native and facade regression tests. |
| JavaScript facade | `textContent`, `nodeValue`, and `TextNode.data` delegate to native operations. Native errors expose invalid placement, and runtime typings restrict child kinds and writable text properties. | No new JavaScript text API is needed for shaping; framework fixtures must use explicit text elements. |
| Publication | `DomPublisher` atomically publishes immutable `PublishedDom { snapshot, changes }` values. `ChangeSet` currently has only `FullRebuild`. | All MTS text work must consume one retained publication. A full recollection per committed revision is the correctness-first invalidation strategy. |
| Typography | `TextStyle`, `ComputedTextStyle`, `FontWeight`, `LineHeight`, `TextWrap`, and validated setters exist in `ui/elements/styles/text.rs`. | Reuse these authoritative values and add only MTS conversion code. Do not put Parley types in DOM nodes or snapshots. |
| Taffy | The low-level trait engine reconciles one immutable publication, measures outer text leaves through the Parley engine, propagates baselines, and retains exact final-width paragraphs. | Keep each computed layout and its final paragraphs revision-matched while the host supplies live viewports; a presented frame may lag that computed revision. |
| Vello | `vello_hybrid` `0.2.0` has its `text` feature enabled. `ui/text/paint.rs` converts retained Parley runs to Glifo/Vello glyph submissions without reshaping or copying font bytes. | The scene host invokes the adapter with renderer-owned `Resources`. |
| Host integration | `Burokku::builder()` assembles native Window ownership, live viewports, layout, scene planning, WGPU surfaces, and presentation. | Input-to-JavaScript dispatch remains Problem 9. |

The current `NodeRevisions::{structure, style, content}` values do not summarize
descendant changes into a paragraph root. The initial implementation must not
use them to decide whether a paragraph is clean. Recollecting from every
`FullRebuild` publication and comparing a complete text/style fingerprint gives
correct invalidation for descendant text, nested style, insertion, removal, and
reparenting changes.

## Scope

### In scope

- enforce that raw text nodes can be attached only beneath `<text>` elements;
- classify outer text elements as the only renderable paragraph sources;
- flatten nested styled text into inherited UTF-8 runs;
- shape and line-break those runs with Parley;
- cache reusable shaping and bounded width variants on MTS;
- represent each paragraph source as one measured Taffy leaf;
- resolve a final shaped layout from the actual Taffy content-box width;
- submit that exact layout's glyphs to Vello Hybrid;
- retain one publication within each candidate computation and revision-tag the
  computed layout, candidate scene, and successfully presented frame
  independently;
- add deterministic unit, integration, and structural rendering tests.

### Out of scope for the first implementation

- browser inline formatting in arbitrary containers;
- independent Taffy boxes or box/background painting for nested `<text>` spans;
- selection, editing, carets, accessibility ranges, ellipsis, decorations,
  hyphenation, web fonts, and synchronous BTS layout queries;
- incremental DOM change batches;
- scale-dependent shaping or physical-pixel quantization;
- multi-window behavior.

There are no anonymous or standalone text leaves. These are the intended forms:

```text
valid:   <div><text>Hello <text>nested style</text></text></div>
invalid: <div>Hello</div>
```

`app.createTextNode(...)` may still create a detached wrapper, but insertion of
that node under `App`, `Window`, `Div`, `Flex`, or `Grid` must fail atomically.
The caller must place string content inside an explicit `<text>` element.

## Architectural decisions

### 1. MTS owns all computed text state

The publication boundary remains:

```text
BTS mutable Dom
    -> immutable PublishedDom
    -> MTS paragraph inputs
    -> MTS Parley layouts and cache
    -> MTS Taffy tree and computed positions
    -> MTS Vello scene
```

`FontContext`, `LayoutContext`, Parley layouts, Taffy IDs, Vello resources, and
cache state must never be stored in `Dom`, `Node`, `DomSnapshot`, or
`PublishedDom`. They should use `Rc` or ordinary MTS-owned values rather than
thread-safe wrappers unless another requirement proves cross-thread ownership is
necessary.

### 2. Text sources are explicit

The only renderable text source is an `Element::Text` whose parent is not
another `Element::Text`. It is a paragraph root and owns one Taffy leaf. All
nested `Element::Text` nodes and raw descendants are consumed by that leaf and
receive no Taffy ID.

A raw `NodeKind::Text` must always have an `Element::Text` parent when attached.
A reconciler that encounters attached raw text anywhere else must return a typed
DOM-invariant error rather than inventing default typography.

Maintain both:

```text
renderable source NodeId -> Taffy NodeId
consumed descendant NodeId -> renderable source NodeId
```

The second map makes ownership and later incremental invalidation unambiguous,
even though the first implementation performs full rebuilds.

### 3. Paragraph input keeps a base style

The collector should produce an owned, MTS-only value similar to:

```rust
struct ParagraphInput {
    source: NodeId, // outer Element::Text paragraph root
    base_style: ComputedTextStyle,
    text: String,
    runs: Vec<StyledTextRun>,
    fingerprint: TextFingerprint,
}

struct StyledTextRun {
    range: Range<usize>,
    style: ComputedTextStyle,
}
```

`base_style` is required even when `text` and `runs` are empty. It lets Parley
receive the paragraph root's font and line-height declarations without inventing
a non-empty run. Non-empty runs must cover the complete UTF-8 text buffer in
order, without overlap or gaps, and adjacent equal styles must be merged.

### 4. Full rebuild is the first invalidation mechanism

For every new `ChangeSet::FullRebuild` target revision:

1. classify and recollect every reachable text source;
2. calculate its complete input fingerprint;
3. reuse cached shaping only when source, complete input, font generation,
   shaping configuration, and width constraint match;
4. rebuild the Taffy representation;
5. remove cache entries for sources no longer reachable.

This correctly invalidates an enclosing paragraph when any descendant text,
style, or structure changes. Do not add a partial `text_dirty` batch until the
repository has a real incremental `ChangeSet` variant.

### 5. Cache shaping separately from width-specific line layout

Parley can re-line-break a shaped `Layout` after cloning it. Use a two-level
cache per source:

```text
(source, complete input, font generation, shaping config)
    -> unbroken shaped Parley Layout
    -> small LRU of line-broken variants by TextConstraint
```

Use a width key that preserves available-space semantics:

```rust
enum TextConstraint {
    MinContent,
    MaxContent,
    Definite(CanonicalFiniteF32Bits),
}
```

Canonicalize negative zero to zero and reject negative, NaN, and infinite finite
widths. Keep one active input/font/config entry and two to four width variants
per source initially; replacing the input drops that source's old variants.
Cache entries may be reused across DOM revisions when their complete inputs are
equal, but the final-paragraph map and computed frame must always carry the
publication revision they represent.

A fingerprint is an accelerator, not proof of equality. Store the canonical
paragraph input with the cache entry and compare it after a fingerprint hit so a
hash collision cannot produce stale text.

### 6. Measurement and painting share one final layout

Taffy `0.11` may probe a measured leaf more than once, and it may skip the
measure callback when both dimensions are already known. Therefore, the last
layout requested by the callback does not identify the final paint paragraph.

Use two steps:

1. The measure callback shapes or reuses variants needed for Taffy's intrinsic
   probes and returns content dimensions.
2. After `compute_layout_with_measure`, traverse all text leaves, read each
   final `taffy::Layout::content_box_width()`, obtain the matching finite-width
   Parley variant, and store that exact retained layout in the computed frame's
   final-paragraph map.

Painting reads only the final-paragraph map. It never reshapes and never guesses
which measurement probe became final.

### 7. Logical coordinates remain unscaled

For the first implementation:

- Parley scale is `1.0`;
- Parley quantization is `false`;
- Taffy rounding is disabled;
- all cache widths and positions are logical pixels;
- the renderer applies the logical-to-physical root transform once.

Display scale is therefore not a text-cache key yet. Add it only if shaping or
quantization later becomes scale-dependent.

### 8. Computed-layout replacement is atomic on MTS

Build reconciliation, layout, and final paragraph resolution into a temporary
computed value tagged with the target revision. Replace the last valid
`ComputedLayout` only after those layout-stage steps succeed. A shaping, Taffy,
or final-paragraph error must not mark the target revision as computed.

Scene construction and presentation are later, separately tagged stages. They
may fail after a newer `ComputedLayout` has been installed; that does not roll
back the computed layout or the authoritative DOM publication. The persistent
text cache may retain individually valid entries created before a later step
failed, while the last successfully presented scene and hit-test plan remain
active where the existing surface is still usable.

## Proposed module layout

```text
crates/burokku/src/ui/
├── layout.rs
├── layout/
│   ├── reconcile.rs     # snapshot -> full Taffy tree and source maps
│   ├── measure.rs       # Taffy callback and final paragraph resolution
│   └── computed.rs      # revision-tagged temporary/committed MTS state
├── text.rs
└── text/
    ├── collect.rs       # source classification, inheritance, UTF-8 runs
    ├── engine.rs        # Parley contexts and style translation
    ├── cache.rs         # unbroken entries and bounded width variants
    ├── paint.rs         # Parley positioned glyphs -> Vello glyph runs
    └── error.rs         # typed collection, shaping, and paint errors
```

The exact split may be adjusted to avoid tiny files, but preserve these
boundaries:

- collection depends only on immutable DOM/style data;
- shaping and cache do not depend on Vello;
- Taffy measurement calls the text engine but does not paint;
- painting consumes an already-resolved final layout and cannot shape;
- renderer-owned `vello_hybrid::Resources` remains outside the text engine.

`crates/burokku/src/ui.rs` should expose these modules only at the minimum
crate-private visibility needed by the future host.

## Detailed implementation stages

### Prerequisite: enforce the strict text content model

**Status: implemented.** The authoritative DOM was corrected before adding
layout fallbacks:

1. In `NodeKind::accepts`, remove `NodeKind::Text` from the children accepted by
   `Window`, `Div`, `Flex`, and `Grid`. Those containers continue to accept
   `Element::Text`.
2. Keep `Element::Text` accepting only raw `NodeKind::Text` and nested
   `Element::Text` children.
3. Restrict `Dom::set_text_content`: update a raw text node in place; replace the
   children of `Element::Text` with one raw text node; return
   `TextContentNotSupported` for `App` and every non-text element.
4. Keep `text_content` getters able to concatenate valid descendant text through
   nested elements.
5. Update the JavaScript facade error tests so appending a `TextNode` directly to
   an ordinary container throws without partial mutation.
6. In `packages/runtime/src/index.ts`, make generic `Node.textContent` and
   `Node.nodeValue` read-only. Redeclare writable `textContent` on `TextElement`
   and writable `textContent`, `nodeValue`, and `data` on `TextNode`.
7. Parameterize or specialize mutation typings by allowed child kind so
   `TextElement` accepts `TextNode | TextElement`, ordinary containers accept
   element children but not `TextNode`, `AppNode` accepts `WindowElement`, and
   `TextNode` accepts no children. Add negative compile tests for
   `div.appendChild(textNode)`.
8. Replace tests and fixtures that currently build `window -> TextNode` or
   `div -> TextNode` with `window/div -> TextElement -> TextNode`.

Do not automatically wrap an invalid raw node in a `<text>` element: that would
change JavaScript-visible structure and wrapper identity. Relationship errors
must be rejected at the attempted mutation.

**Exit criteria**

- `<text>plain <text>nested</text></text>` is valid;
- `<div><text>plain</text></div>` is valid;
- `<div>plain</div>` and the equivalent `div.appendChild(TextNode)` are rejected;
- failed insertion and failed non-text `textContent` assignment leave the tree,
  node revisions, and publication revision unchanged;
- a detached `TextNode` remains usable and may later be attached beneath a
  `<text>` element.

### Stage 0: dependency and API compatibility spike

**Status: implemented.**

Update `crates/burokku/Cargo.toml` in a small, compile-tested change:

- add and lock Parley `0.11.1`;
- enable `vello_hybrid`'s `text` feature in addition to `wgpu`;
- add a direct `glifo` `0.3.0` dependency because Vello's glyph builder consumes
  `glifo::Glyph` values and Vello does not re-export that type;
- keep Taffy on the existing `0.11` line.

Before building the full pipeline, compile a private compatibility adapter that:

1. creates reusable `parley::FontContext` and `LayoutContext<[u8; 4]>` values;
2. registers an embedded font and shapes one word in a normal unit test;
3. iterates `PositionedLayoutItem::GlyphRun`;
4. type-checks passing `run.font()` to `Scene::glyph_run` without copying font
   bytes;
5. adapts positioned Parley glyphs to `glifo::Glyph`.

The Vello portion can be a function compiled against borrowed `&mut Scene` and
`&mut Resources`; `Resources` is created by `vello_hybrid::Renderer`, so do not
invent a second resource constructor merely to run this spike. Exercise the
submission at runtime once the headless or window renderer fixture exists.

At these versions, Parley and Peniko resolve
`linebender_resource_handle::FontData` `0.1.1`, so the font handle should be the
same concrete type. Keep the compile test to detect future dependency drift.
Also verify Glifo support for Parley's normalized variation coordinates and font
synthesis before freezing the paint adapter.

Add a small redistributable font under, for example,
`crates/burokku/testdata/fonts/`, together with its license. Tests must register
that font explicitly and select its family by name; system-font metrics are not
a test oracle.

**Exit criteria**

- the new feature set compiles on the workspace toolchain;
- one deterministic Parley layout contains positioned glyphs and the Vello
  submission adapter type-checks against borrowed renderer resources;
- no duplicate incompatible `linebender_resource_handle` versions appear in
  `cargo tree`.

### Stage 1: paragraph collection and fingerprints

**Status: implemented.**

Implement `ui/text/collect.rs` against `DomSnapshot`.

Collection algorithm for an outer text element:

1. read its `TextElementStyle::text` and resolve it against user-agent defaults;
2. traverse descendants iteratively in DOM order;
3. when entering a nested text element, resolve its declarations against the
   active computed style;
4. append raw strings to one UTF-8 buffer;
5. record byte ranges with the active computed style;
6. merge adjacent equal styles;
7. validate complete coverage and UTF-8 boundaries;
8. fingerprint the base style, text bytes, ranges, and every computed style
   field using explicit enum tags and canonical `f32::to_bits()` values, with
   negative zero normalized to zero.

Every collected raw node inherits the active computed style of its enclosing
text element. The iterative stack should carry owned or cloned computed styles,
and the finished `ParagraphInput` must own everything it needs rather than
borrow the snapshot. Collection must not recurse on the Rust call stack.

Add a classification helper used by both collection and Taffy reconciliation so
those modules cannot disagree about which nodes are consumed.

**Tests**

- nested inheritance and one-property overrides;
- sibling and deeply nested DOM order;
- adjacent equal-run merging;
- UTF-8 text containing combining marks, emoji, RTL text, and CJK;
- empty roots and empty nested spans;
- a deep adversarial tree without stack overflow;
- moving a nested span between parents changes computed style and fingerprint;
- box styles on nested spans do not create inline boxes or affect the paragraph
  input.

### Stage 2: reusable Parley engine and cache

**Status: implemented.**

Implement an MTS-only `TextEngine` containing:

```rust
struct TextEngine {
    font_context: parley::FontContext,
    layout_context: parley::LayoutContext<TextBrush>,
    font_generation: u64,
    cache: TextLayoutCache,
}

type TextBrush = [u8; 4];
```

Keep font mutation behind engine methods so successful registration/removal can
increment `font_generation` and invalidate relevant entries. The initial system
font collection may be treated as static after engine construction; dynamic
font discovery and web fonts remain out of scope.

Translate every `ComputedTextStyle` field:

| Burokku | Parley |
| --- | --- |
| `font_family` | `FontFamily::Source` using the authoritative non-empty family string |
| `font_size` | `StyleProperty::FontSize` |
| `font_weight` | `parley::FontWeight` |
| `color` | `StyleProperty::Brush([r, g, b, a])` |
| `LineHeight::Normal` | `parley::LineHeight::MetricsRelative(1.0)` |
| `LineHeight::Factor(n)` | `FontSizeRelative(n)` |
| `LineHeight::Length(px)` | `Absolute(px)` |
| `TextWrap::Wrap` | `TextWrapMode::Wrap` |
| `TextWrap::NoWrap` | `TextWrapMode::NoWrap` |

Push the base properties as Parley defaults, then apply full computed styles to
all non-empty UTF-8 ranges. Build one unbroken layout per complete input and
clone it for line breaking:

- `MaxContent`: `break_all_lines(None)`;
- `Definite(width)`: `break_all_lines(Some(width))`;
- `MinContent`: use `calculate_content_widths().min`, then line-break at that
  non-negative width.

Call `align(Alignment::Start, ...)` consistently after line breaking. Use
`Layout::width()` (which excludes trailing whitespace) and `Layout::height()` as
the measured content size, and verify that choice with whitespace tests.
Preserve Parley's native empty-text result and lock it down with a
characterization test; do not give measurement and painting separate empty-text
special cases.

Return typed errors for non-finite dimensions, malformed range coverage,
invalid metrics, and styled-run resource limits. Paragraph collection is
iterative and does not impose a separate nesting-depth limit. Accepted DOM
input must not intentionally unwind the renderer. When font resolution produces
no glyphs for a non-whitespace paragraph, measure and paint explicit replacement
boxes instead of failing the frame. Do not insert failed results into the
cache.

**Tests**

- all six style fields reach Parley;
- a finite narrow width wraps and increases line count/height;
- `nowrap` does not soft-wrap;
- min-content and max-content widths are stable;
- matching calls reuse the same retained width variant;
- different finite-width bits produce different variants;
- the per-source LRU evicts its oldest variant at the configured bound;
- text, inherited style, color, font generation, and shaping-config changes miss
  the appropriate cache entry;
- unchanged input can reuse shaping across two DOM revisions;
- deterministic metrics use only the embedded test font.

### Stage 3: text-aware full Taffy reconciliation

**Status: implemented using Taffy's low-level trait API.**

Implement the first full-rebuild reconciler as a coordinated subset of problem
7. It should consume one `&PublishedDom` and an explicit logical viewport.

Representation rules:

- `App` is not a Taffy node;
- the attached `Window` is the per-window layout root;
- ordinary elements become Taffy containers using their existing authoritative
  style conversions;
- an outer `Element::Text` becomes one leaf with a text node context and no
  Taffy children;
- nested text elements and their raw descendants are recorded in the owner map
  but skipped in the Taffy tree;
- attached raw text outside a text element is a typed invariant error, never a
  fallback Taffy leaf;
- child order follows the committed snapshot exactly.

Each paragraph `LayoutRole` retains an `Rc<ParagraphInput>`. The temporary
low-level Taffy trait adapter borrows `&mut TextEngine`; its `compute_leaf_layout`
measurement closure returns Parley's content size only. Taffy remains
responsible for margin, padding, border, scrollbar insets, explicit dimensions,
and the final box.

Handle width modes explicitly and defensively:

- `AvailableSpace::MaxContent` uses the max-content variant;
- `AvailableSpace::MinContent` uses the min-content variant;
- `AvailableSpace::Definite(width)` uses a canonical non-negative finite width;
- known dimensions remain Taffy's authority. Use the content width reflected in
  `available_space` to line-break text needed for an unknown height.

Run with Taffy rounding disabled. Compute into temporary topology and sidecars;
replace the current computed frame only after reconciliation, shaping, layout,
final paragraph resolution, and finite-value validation succeed. The low-level
trait adapter writes Parley's first typographic baseline into
`LayoutOutput::first_baselines`, including the outer paragraph's top border and
padding exactly once.

**Tests**

- an outer text element and all nested spans produce exactly one Taffy leaf;
- raw text outside a text element is rejected by DOM mutation and defensively
  rejected by reconciliation;
- block, flex, and grid parents lay out explicit text-element children;
- narrow definite width changes paragraph height;
- padding contributes exactly once;
- explicit width and height remain controlled by Taffy;
- the pinned Taffy measured-leaf baseline fallback is characterized and
  documented;
- repeated Taffy probes reuse matching text-cache variants;
- removed/reclaimed and newly detached sources are absent from the rebuilt maps;
- a retained old `PublishedDom` can finish layout while a newer revision is
  published.

### Stage 4: final paragraph resolution and revision-safe computed frames

**Status: implemented.**

After Taffy computes layout:

1. traverse every mapped text leaf;
2. read its final content-box width from the Taffy layout;
3. retrieve or create the exact matching finite-width Parley variant;
4. retain it in `final_paragraphs: HashMap<NodeId, Rc<ShapedParagraph>>`;
5. record the target DOM revision on the computed frame;
6. prune cache sources not present in the new reachable source set.

The computed frame should expose enough read-only data for scene construction:

```text
revision
DOM NodeId -> Taffy NodeId
final Taffy layouts
text source -> final shaped paragraph
text descendant -> source owner
```

Do not expose a method that asks the engine to shape during painting. Assert that
the publication revision, computed-layout revision, and later scene revision
match at each boundary.

**Tests**

- the final paragraph matches the final content width rather than merely the
  last measurement callback;
- a leaf with both explicit dimensions still receives a paintable final
  paragraph even if Taffy skipped measurement;
- a failed final reshape leaves the preceding computed frame current;
- deleting a paragraph evicts its source entries;
- NodeId generation reuse cannot retrieve a reclaimed node's cache entry.

### Stage 5: Vello Hybrid paint adapter

**Status: implemented and invoked by the native scene host.**

Implement `ui/text/paint.rs` as an adapter from the retained Parley layout to
renderer-owned Vello state. It should accept an existing
`&mut vello_hybrid::Scene`, `&mut vello_hybrid::Resources`, a logical
content-box origin, and `&ShapedParagraph`.

For each `PositionedLayoutItem::GlyphRun`:

1. use `positioned_glyphs()`; it already includes Parley's run offset and
   baseline, so do not add either a second time;
2. convert each item to `glifo::Glyph { id, x, y }`, adding only the paragraph's
   logical content-box origin;
3. set Vello's solid paint from the Parley run brush;
4. pass the shared `FontData`, font size, normalized variation coordinates, and
   required font synthesis to `Scene::glyph_run`;
5. call `fill_glyphs` with the positioned iterator.

The renderer, not `TextEngine`, owns and reuses `vello_hybrid::Resources` so the
glyph preparation and atlas caches survive scene rebuilds. Do not copy font
bytes per run or per frame.

No element overflow/clip style exists in the current authoritative style model.
The first adapter therefore respects the renderer/window clip and does not
invent span clipping. Add ancestor clipping only when a corresponding DOM style
is introduced.

Prefer structural assertions over screenshots. A small internal glyph-batch
preparation layer or test sink may expose IDs, positions, brush, font handle,
and variation data without depending on private Vello scene internals.

**Tests**

- non-empty text emits at least one glyph;
- Unicode shaping preserves positioned glyph order;
- nested run colors produce the expected separate glyph submissions;
- font size, weight, font selection, variations, and synthesis reach Glifo;
- content origin includes Taffy padding exactly once;
- the first baseline is not applied twice;
- the adapter consumes the exact final `Rc<ShapedParagraph>` resolved after
  Taffy;
- a small headless pixel smoke test uses the embedded font and explicit
  tolerances when GPU test infrastructure is available.

### Stage 6: host and redraw integration

This stage lands with the application host, native window lifecycle, and Vello
renderer workstreams rather than creating a second temporary host.

For each update and redraw:

1. observe one latest `Arc<PublishedDom>` independently from applying its native
   `WindowSpec`, and retain it as the content target;
2. obtain the actual logical viewport from the active native `Window`;
3. reconcile and lay out only when the target revision or viewport requires it;
4. resolve final paragraphs and install only a complete `ComputedLayout`;
5. build backgrounds and text into a candidate Vello scene from that computed
   revision;
6. render/present and record the candidate revision only after success;
7. if a newer commit arrived during the frame, schedule another redraw and
   coalesce directly to the newer publication rather than switching snapshots
   mid-frame.

The commit notifier runs after publication and redraw demand is coalesced. The
latest DOM publication remains authoritative even when rendering it fails; no
DOM rollback or rejected-revision registry is used. After a frame has been
presented, layout, text, or scene candidate failures are recorded with their
revision and stage while the last successfully presented scene/hit-test plan
remains active. The unchanged failing target is not immediately retried. Before
the first successful frame, the same failures remain fatal because there is no
usable UI to retain.

**Integration tests**

- the completion example in `text_rendering_plan.md` creates a non-empty glyph
  scene;
- one macrotask plus ready microtasks yields one publication and one redraw;
- changing `TextNode.data`, `textContent`, or a nested text style changes the
  next presented revision;
- a commit during scene construction is deferred to the next frame;
- each successfully presented frame's measurement, painting, hit-test data, and
  presentation record one revision even when it lags the latest publication;
- removing the final paragraph clears its Taffy and text-cache state.

### Stage 7: incremental invalidation and performance follow-up

Only after an incremental `ChangeSet` exists:

- add `text_dirty` paragraph sources and removed sources to the bounded batch;
- on raw text data or text-element style mutations, resolve and mark the
  enclosing paragraph root;
- on moves, insertions, removals, and replacements, dirty both old and new text
  owners as applicable;
- fall back to full rebuild when the source revision is skipped or a batch is
  unavailable;
- consider splitting color-only paint data from glyph shaping after profiling.

Benchmark cold shaping, warm shaping, width changes, cache eviction, paragraph
collection, and scene glyph counts. Add global cache budgets and adversarial
input limits based on measurements rather than unbounded retention.

## File-level change summary

| File/path | Planned change |
| --- | --- |
| `crates/burokku/Cargo.toml` | Add Parley and Glifo; enable Vello Hybrid `text`. |
| `crates/burokku/src/ui.rs` | Register crate-private `text` and `layout` modules. |
| `crates/burokku/src/ui/text.rs` and `ui/text/*` | Add collection, shaping, cache, errors, and paint adapter. |
| `crates/burokku/src/ui/layout.rs` and `ui/layout/*` | Add the full-rebuild Taffy representation, measurement callback, source maps, and revision-tagged computed state. |
| `crates/burokku/src/ui/elements.rs` | **Completed foundation:** strict child matrix, text-only `set_text_content`, and rejection/no-op tests. Later, optionally add one centralized element-to-Taffy-style helper. |
| `packages/runtime/src/index.ts` | **Completed foundation:** read-only generic text properties, writable text wrappers, and child mutation types matching the native matrix. |
| `crates/burokku/src/ui/elements/publication.rs` | No first-pass protocol change; continue consuming `FullRebuild`. Extend only with the later incremental batch. |
| `crates/burokku/src/ui/dom_plugin*` | **Completed foundation:** strict placement errors and explicit text-element fixtures. Add no rendering responsibility. |
| `crates/burokku/testdata/fonts/*` | Add a deterministic licensed test font and its license. |
| future host/renderer modules | Supply viewport, retain publications, own Vello resources, build scenes, and present revision-tagged frames. |

## Validation commands

Run focused checks after each stage and the complete workspace gate before
calling the workstream complete:

```bash
cargo fmt --check
cargo test -p burokku --lib
cargo test -p burokku text
cargo clippy -p burokku --all-targets --all-features -- -D warnings
pnpm --filter @burokku/runtime build
cargo test --workspace
```

## Completion criteria

Problem 6 is complete for the initial contract when all of the following hold:

- raw text can be attached only beneath a text element, while text elements may
  nest recursively;
- direct string children of `App`, `Window`, `Div`, `Flex`, and `Grid` are
  rejected without mutation;
- every reachable outer text element owns one measured Taffy leaf;
- nested text elements produce inherited UTF-8 runs, not independent boxes;
- Taffy measures text from Parley under min-content, max-content, and finite
  constraints without double-counting padding;
- cache reuse is bounded and keyed by complete input, width mode, font
  generation, and shaping configuration;
- final scene construction uses the exact Parley layout resolved from Taffy's
  final content-box width;
- font data is shared without per-frame byte copies;
- every computed layout, candidate scene, and presented frame carries the
  revision that produced it, while those stage revisions may temporarily
  differ as rendering lags or coalesces publications;
- a candidate failure leaves the latest DOM authoritative and, where the active
  surface remains usable, retains the last successfully presented scene and
  hit-test plan without rolling back the DOM;
- the example program in `text_rendering_plan.md` renders and updates visible
  glyphs end to end.
