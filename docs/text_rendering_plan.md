# Text rendering implementation plan

## Status

This document plans the complete text path. It does not add Parley or a shaping
module yet. Those dependencies and modules should be introduced when Taffy
measurement and Vello glyph rendering are implemented together.

The current foundation already has:

- raw DOM text as `NodeKind::Text(String)`;
- styled text containers as `Element::Text`;
- `TextElementStyle`, inherited `TextStyle`, and `ComputedTextStyle` in
  `crates/burokku/src/ui/elements/styles/text.rs`;
- validated style properties for font family, size, weight, color, line height,
  and wrapping.

The missing work is DOM text behavior, paragraph/run collection, shaping,
measurement, caching, and painting.

## Goals

1. `textContent` and raw text updates have explicit DOM semantics.
2. Nested `<text>` elements become inherited styled runs in one paragraph.
3. Parley shapes each paragraph using the width constraints supplied by Taffy.
4. Taffy receives correct intrinsic width and height for text leaves.
5. Vello renders the exact Parley layout used during measurement.
6. Text, style, width, and font changes invalidate the correct cached state.
7. Every frame uses text, layout, and paint data from one committed DOM
   revision.

## Non-goals for the first implementation

- Full browser inline formatting inside arbitrary block elements.
- Selection, editing, carets, or accessibility text ranges.
- Ellipsis, text overflow, justification, decorations, or inline images.
- Web font loading or CSS font-face rules.
- Hyphenation and every CSS white-space mode.
- Synchronous BTS layout queries.

These features should not be implied by the TypeScript API until implemented.

## Required ownership boundary

The text pipeline follows the existing dual-runtime design:

```text
BTS mutable DOM
    -> immutable committed snapshot
    -> MTS paragraph collection
    -> MTS Parley shaping
    -> MTS Taffy measurement/layout
    -> MTS Vello scene construction
```

The DOM snapshot contains strings and authoritative styles only. Parley
contexts, shaped layouts, Taffy nodes, font caches, and Vello resources remain
MTS-owned and must not be published through `ArcSwap`.

A frame must retain one committed snapshot for paragraph collection, layout,
and scene construction. A newer commit is handled by a later frame.

## Text tree contract

### Outermost styled text

An `Element::Text` whose parent is not another `Element::Text` is a paragraph
root. It owns one Taffy leaf and one shaped Parley layout.

Its `TextElementStyle::common` controls the paragraph box. Its resolved
`TextStyle` supplies the base typography.

### Nested styled text

An `Element::Text` beneath another `Element::Text` is a styled span. It does not
receive an independent Taffy node. Its text declarations inherit from and
optionally override its enclosing computed style.

For the first implementation, box properties on nested `<text>` spans do not
create inline boxes. Width, height, margin, padding, and background paint on a
nested span are outside the initial contract. This limitation must be tested
and documented rather than silently treated as a second block.

### Raw text under a non-text element

The current DOM permits raw text beneath `Window`, `Div`, `Flex`, and `Grid`.
Initially, each such raw text node becomes a standalone text leaf using
`ComputedTextStyle::default()`. It is keyed by the raw node's `NodeId` and
participates in the parent's normal Taffy child order.

This provides deterministic behavior without implementing a browser inline
formatting context. A later implementation may group adjacent raw text into
anonymous paragraphs.

### Empty text

An empty paragraph remains a valid Taffy leaf. Its exact line-height behavior
must be chosen and tested against Parley. The result must remain stable across
measurement and rendering; the renderer must not independently special-case it.

## DOM APIs and mutation semantics

Add native operations before connecting JavaScript wrappers:

```rust
Dom::text_content(id) -> Result<String, DomError>
Dom::set_text_content(id, value) -> Result<bool, DomError>
Dom::set_text(text_node_id, value) -> Result<bool, DomError>
```

Required behavior:

- A getter on a raw text node returns its data.
- A getter on an element concatenates descendant raw text in tree order.
- A setter on a raw text node updates that node in place.
- A setter on an element detaches its existing children and inserts one raw
  text child containing the assigned value.
- Replaced children remain valid while retained by JavaScript wrappers. The
  setter must not permanently reclaim them.
- Assigning the same effective value is a no-op where structure does not need
  to change.
- `nodeValue` and `data` update only existing raw text nodes.

The future QuickJS DOM facade maps JavaScript `textContent`, `nodeValue`, and
`Text.data` to these operations. `title.textContent = "Click counter"` must
therefore construct a raw child synchronously in the BTS staging DOM.

## Paragraph collection

Add an MTS-only collector, for example:

```text
crates/burokku/src/ui/text/
├── mod.rs
├── collect.rs
├── shape.rs
├── cache.rs
└── paint.rs
```

The collector converts a paragraph root into:

```rust
struct ParagraphInput {
    source: NodeId,
    text: String,
    runs: Vec<StyledTextRun>,
    fingerprint: TextFingerprint,
}

struct StyledTextRun {
    range: Range<usize>,
    style: ComputedTextStyle,
}
```

Collection algorithm:

1. Resolve the paragraph root's `TextStyle` against user-agent defaults.
2. Traverse descendants in DOM order.
3. Resolve each nested `<text>` style against its enclosing computed style.
4. Append every raw text string to one UTF-8 buffer.
5. Record byte ranges using the computed style active for each raw string.
6. Merge adjacent ranges with equal computed styles.
7. Validate that all ranges are ordered, non-overlapping, and on UTF-8
   boundaries.

Use byte ranges because Parley ranged styles use UTF-8 byte offsets. Never
calculate these ranges from JavaScript UTF-16 indices.

The collector should be iterative or enforce a depth limit so an adversarial
DOM cannot overflow the Rust stack.

## Parley integration

Introduce Parley only when the measurement phase starts. Select and pin a
version compatible with the project's Rust toolchain and Vello dependency;
Parley `0.11` is the initial candidate.

Create one reusable MTS engine:

```rust
struct TextEngine {
    font_context: parley::FontContext,
    layout_context: parley::LayoutContext<TextBrush>,
    cache: TextLayoutCache,
}

type TextBrush = [u8; 4];
```

`FontContext` and `LayoutContext` must be reused rather than constructed for
each node or frame. A simple RGBA brush keeps Parley independent of renderer
paint types.

Translate each `ComputedTextStyle` as follows:

| Burokku | Parley |
| --- | --- |
| `font_family` | `FontFamily` |
| `font_size` | `StyleProperty::FontSize` |
| `font_weight` | `FontWeight` |
| `color` | RGBA `Brush` |
| `LineHeight::Normal` | metrics-relative line height |
| `LineHeight::Factor(n)` | font-size-relative line height |
| `LineHeight::Length(px)` | absolute line height |
| `TextWrap::Wrap` | `TextWrapMode::Wrap` |
| `TextWrap::NoWrap` | `TextWrapMode::NoWrap` |

The shaping operation receives `ParagraphInput` and a width constraint, pushes
base and ranged properties, builds a `parley::Layout`, and performs line
breaking. It returns a retained layout plus its width, height, and baseline
metrics.

Invalid scale, width, font size, line height, or run boundaries must return a
typed error rather than enter a cache.

## Logical coordinate policy

Taffy layout uses logical pixels. The first implementation should shape in
logical coordinates with Parley scale `1.0` and quantization disabled. The
window/Vello root transform converts logical coordinates to physical pixels.

This avoids dividing Parley metrics by display scale and prevents accidental
double scaling. Pixel quantization and scale-aware reshaping may be introduced
later only with tests covering fractional display scales.

## Taffy reconciliation and measurement

### Taffy representation

- An outermost `Element::Text` is represented by one measured Taffy leaf.
- Nested text elements and their raw children are consumed by that leaf and do
  not receive separate Taffy nodes.
- A raw text node directly under a non-text element is represented by one
  measured Taffy leaf using default typography.

The reconciler needs a mapping from each renderable text source `NodeId` to its
Taffy leaf and cached paragraph state.

### Measurement callback

Use Taffy's measured-leaf API during layout. Conceptually:

```text
Taffy asks to measure text leaf
    -> derive content width constraint
    -> collect or reuse ParagraphInput
    -> retrieve or shape Parley layout for that width
    -> return content width and height to Taffy
```

Taffy, not Parley, remains responsible for paragraph padding, margin, explicit
box dimensions, parent constraints, and final position.

Handle Taffy available-space modes explicitly:

- `MaxContent`: shape without a maximum width.
- `Definite(width)`: shape using the non-negative content-box width.
- `MinContent`: use Parley's content-width information or shape at its minimum
  legal wrap width; verify the chosen behavior with Taffy tests.
- Known width or height supplied by Taffy overrides the corresponding measured
  dimension as required by the measured-leaf contract.

Subtract Taffy-managed border and padding only if the callback receives a
border-box width. Do not add padding to Parley's metrics and then let Taffy add
it again.

Taffy may call the callback repeatedly with different constraints. The callback
must be deterministic and must not mutate DOM state or request publication.

## Shaped-text cache

Store shaped layouts in MTS computed state, never in authoritative DOM nodes.
A cache key must include at least:

```text
paragraph source NodeId
paragraph text/style fingerprint
available content width mode and finite width bits
font collection generation
shaping configuration version
```

With the initial logical-coordinate policy, display scale is not part of the
key. Add it if shaping later becomes scale-dependent.

Correctness-first invalidation:

- Hash the flattened text plus computed runs to produce `TextFingerprint`.
- A changed fingerprint cannot reuse a previous shaped layout.
- A changed finite width cannot reuse a wrapped layout from another width.
- Font database changes invalidate all affected paragraphs.
- Removed paragraph sources remove their cache entries.

Bound the number of cached width variants per paragraph because Taffy may probe
several widths. A small LRU of two to four variants is sufficient initially.

Later, the DOM `ChangeSet` should mark text-dirty paragraph roots directly.
Until descendant dirty propagation exists, fingerprinting during reconciliation
is the correctness fallback.

## Vello Hybrid rendering

Enable Vello Hybrid's `text` feature when this phase starts; the current
`default-features = false` configuration does not enable glyph rendering.

Scene construction uses the same cached Parley layout selected by Taffy. It
must not reshape text independently.

For each Parley line and positioned glyph run:

1. Read the run font, font size, normalized variation coordinates, brush, and
   positioned glyphs.
2. Translate the paragraph content-box origin plus Parley glyph positions into
   scene coordinates.
3. Convert RGBA `TextBrush` into a Vello/Peniko solid color.
4. Submit the run through `vello_hybrid::Scene::glyph_run` and its glyph-run
   builder.
5. Apply clipping from the paragraph and ancestor layout boxes where required.

Parley and Peniko currently re-export `FontData` from
`linebender_resource_handle`. Dependency resolution must keep this resource type
compatible. If versions diverge, add an explicit conversion from the shared font
blob and collection index rather than copying font bytes per frame.

The renderer owns and reuses Vello `Resources` so the glyph atlas and glyph
preparation caches survive scene rebuilds.

### Positioning rule

The paragraph's Taffy content-box origin is the origin for Parley's layout.
Padding is already reflected by the content-box offset. A glyph baseline is:

```text
content-box x + Parley glyph x
content-box y + Parley line/glyph baseline y
```

Define this once in the paint adapter and test it with non-zero margin and
padding to prevent double offsets.

## Invalidation and revisions

The existing `NodeRevisions::content` is insufficient because a descendant raw
text change does not summarize into its paragraph root. Introduce or use a text
change category in the committed `ChangeSet`.

Mutations that make a paragraph text-dirty include:

- raw text data changes;
- insertion, removal, movement, or replacement of a text descendant;
- font, line-height, or wrap changes on any enclosing text span;
- reparenting a span under a different inherited style;
- font collection changes.

Width changes do not require recollecting text runs, but they do require a
width-specific Parley line layout. Paint-only color changes can reuse glyph
shaping in a future split cache, but the first implementation may rebuild the
paragraph layout for correctness.

A failed reshape must not mark the new DOM revision as fully computed. Retain
the last valid presented scene where possible and schedule/report the failure.

## Implementation sequence

### Phase 1: DOM text semantics

- Add native `text_content` and `set_text_content` operations.
- Preserve detached replaced children for live wrapper identity.
- Add raw `nodeValue`/`data` behavior.
- Add subtree concatenation and no-op tests.
- Connect these operations to the future QuickJS DOM facade.

### Phase 2: Paragraph and run collection

- Define paragraph roots and standalone raw text leaves.
- Implement inherited style collection and adjacent-run merging.
- Generate deterministic text/style fingerprints.
- Add UTF-8, nested span, reparenting, empty-string, and deep-tree tests.

### Phase 3: Parley shaping

- Add the pinned Parley dependency.
- Add reusable `TextEngine` contexts.
- Translate every `ComputedTextStyle` field.
- Shape unbounded and finite-width paragraphs.
- Use an embedded, redistributable test font for deterministic metrics.

### Phase 4: Taffy measured leaves

- Reconcile paragraph roots into measured Taffy leaves.
- Implement definite, min-content, and max-content measurement.
- Add bounded width-variant caching.
- Verify padding, explicit dimensions, flex/grid constraints, and wrapping.

### Phase 5: Vello glyph painting

- Enable the Vello Hybrid `text` feature.
- Convert Parley glyph runs and font resources into Vello glyph submissions.
- Render from the measured cached layout.
- Verify content-box offsets, clipping, colors, nested styles, and Unicode.

### Phase 6: Commit integration and invalidation

- Carry text-dirty sources in committed change sets.
- Remove cache entries for deleted nodes.
- Coalesce multiple BTS text edits into one reshape/redraw checkpoint.
- Ensure frame revision, Taffy layout, shaped text, and scene revision match.

### Phase 7: Performance and robustness

- Benchmark shaping cold/warm, wrapping at changing widths, and scene rebuilds.
- Record cache hits, misses, evictions, and shaped glyph counts.
- Bound paragraph length, run count, cache variants, and font fallback work if
  untrusted scripts can create adversarial content.
- Test missing fonts, malformed font data, RTL text, combining marks, emoji,
  CJK wrapping, and very long unbroken strings.

## Required tests

### DOM

- `textContent` getter concatenates nested descendant text in order.
- `textContent` setter leaves one raw child and detaches previous children.
- `nodeValue` updates only raw text nodes.
- Live wrappers to replaced children remain valid and detached.

### Collection and inheritance

- Nested text inherits every omitted property.
- Explicit child properties override only their byte range.
- Adjacent equal runs merge.
- UTF-8 ranges always end on character boundaries.
- Moving a span changes its inherited computed style and fingerprint.

### Layout

- Unbounded text reports stable intrinsic dimensions.
- Narrower definite width increases line count and usually height.
- `nowrap` does not soft-wrap under a narrow parent.
- Padding is counted once.
- Text behaves as a measured child of block, flex, and grid containers.
- Repeated Taffy probes reuse matching width-cache entries.

### Rendering

- The example title produces non-empty glyph submissions.
- Font size, weight, color, and nested run changes affect the expected runs.
- Taffy and rendered paragraph origins agree with non-zero padding.
- Scene construction uses the same shaped-layout cache entry as measurement.
- A text-only update requests one redraw after the BTS checkpoint.

Use structural glyph-run assertions for most tests. Keep a small number of
image or pixel tests for integration, with deterministic embedded fonts and
explicit tolerances.

## Completion criteria

Text rendering is complete for the initial contract when this program works
end to end:

```ts
const title = app.createElement("text");
title.textContent = "Click counter";
setStyles(title, {
  fontFamily: "sans-serif",
  fontSize: "24px",
  fontWeight: "bold",
  color: "#ffffffff",
  lineHeight: 1.2,
  textWrap: "wrap",
});
const mainWindow = app.createElement("window");
mainWindow.appendChild(title);
app.appendChild(mainWindow);
```

The committed text must be collected into styled runs, measured by Parley from
Taffy's width constraint, positioned by Taffy, drawn through Vello Hybrid, and
updated after later `textContent` or style mutations without using stale
shaping data.
