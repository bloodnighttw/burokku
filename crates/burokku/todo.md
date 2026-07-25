# Burokku TODO

## Style correctness

- [x] Apply `z-index` when ordering children for painting and hit testing.
- [x] Implement stacking contexts, including `isolation: isolate`.
- [x] Clip descendants for `overflow: hidden` and `overflow: clip`.
- [x] Implement scroll containers and scrollbars for `overflow: auto` and `overflow: scroll`.
- [x] Preserve and paint per-side border widths instead of using the largest width for every side.
- [x] Support per-side border colors and styles.
- [ ] Propagate Glyphon text baselines into Taffy so baseline alignment is accurate.
- [ ] Recompute `line-height: normal` when the effective font size changes.
- [x] Support elliptical border radii and the `border-radius` slash syntax.
- [x] Distinguish `position: static` from `position: relative`.
- [x] Implement viewport-relative behavior for `position: fixed`.

## Layout

<!--<!--- [ ] Add inline formatting for `span` and other inline elements.-->
<!--- [ ] Preserve inline participation for `inline-flex` and `inline-grid`.-->-->
- [ ] Add grid templates, explicit and implicit track sizing, placement, and auto-flow properties.
- [ ] Add the `flex` shorthand and `order`.
<!--- [ ] Add logical box properties such as `padding-inline`, `margin-block`, and logical insets.-->

## Typography

- [ ] Add `text-align`.
- [ ] Add `font-style`.
- [ ] Add letter and word spacing.
- [ ] Add text decoration.
- [ ] Add whitespace and wrapping controls instead of always using word wrapping.
- [ ] Parse font-family fallback lists.

## Paint

- [ ] Add opacity.
- [ ] Add transforms.
- [ ] Add box and text shadows.
- [ ] Add background images and gradients.
- [ ] Add `rgb()`, `rgba()`, `hsl()`, and `hsla()` colors.
- [ ] Expand named-color support.

## Elements

<!--- [ ] Render image contents and intrinsic image dimensions.-->
- [ ] Add native visual behavior for buttons.
- [ ] Add native visual behavior for selects and options.
