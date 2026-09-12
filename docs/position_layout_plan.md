# Positioned layout plan

## Decision

Keep the DOM tree authoritative and unchanged. Re-parent positioned boxes only in the derived `LayoutTopology` passed to Taffy. DOM APIs, event propagation, text inheritance, and node ownership must continue to use DOM parentage.

```text
DOM tree
   |
   v
reconcile_full() -- resolve effective layout parents
   |
   v
LayoutTopology -- derived Taffy tree
   |
   v
Taffy -> ComputedLayout
```

Taffy exposes only relative and absolute positioning. Burokku therefore lowers its four position values as follows:

| Burokku position | Taffy position | Effective Taffy parent |
| --- | --- | --- |
| `static` | `Relative` | Direct DOM parent |
| `relative` | `Relative` | Direct DOM parent |
| `absolute` | `Absolute` | Nearest non-static ancestor, otherwise the window |
| `fixed` | `Absolute` | Window |

The window remains the viewport-sized root and is forced to Taffy's relative positioning internally.

## Implementation phases

### 1. Complete the style contract

In `crates/burokku/src/ui/elements/styles/position.rs`:

- parse `static`, `relative`, `absolute`, and `fixed`;
- retain the existing conversion from Burokku `Position` to Taffy `Position`;
- reject every other value.

In `crates/burokku/src/ui/elements/styles/common.rs`:

- add `position: Position` to `CommonStyle`;
- default it to `Position::Static`;
- support setting and removing the `position` property;
- assign the converted value to `taffy::Style::position`.

This covers div, flex, grid, and outer text layout boxes because they already share `CommonStyle`. Do not expose `position` on `WindowStyle`.

### 2. Resolve effective parents while lowering the DOM

Update `PendingNode` in `crates/burokku/src/ui/layout/reconcile.rs` to carry:

- the actual DOM parent;
- the DOM parent's layout ID;
- the nearest non-static ancestor's layout ID;
- the window layout ID;
- the existing sibling source order.

For each node:

1. Read both its Burokku position and converted Taffy style.
2. Select the effective layout parent using the table above.
3. Insert the node under that parent in `LayoutTopology`.
4. If the node is relative, absolute, or fixed, pass its layout ID as the nearest positioned ancestor when scheduling its descendants. Otherwise, pass the inherited positioned ancestor through.

The existing depth-first traversal visits ancestors before descendants, so every possible containing block already exists when a child is inserted. Its insertion order also preserves DOM preorder among boxes moved under the same effective parent; no second lowering pass is needed.

### 3. Preserve both relationship graphs

Use the existing separation in `crates/burokku/src/ui/layout/topology.rs`:

- `PositioningMeta::dom_parent` retains authoritative DOM ancestry;
- topology `parent` and `children` hold the effective Taffy tree;
- `PositioningMeta::containing_block` records the selected effective parent;
- `source_order` remains the child's index under its original DOM parent.

Do not mutate `Dom`, and do not teach event dispatch or text collection about layout parents.

No new Taffy adapter is required: `DerivedLayoutTree` already reads children exclusively from `LayoutTopology`. `ComputedLayout::build_boxes` already accumulates coordinates through topology parents, so it should produce viewport-space origins for re-parented boxes without another coordinate pass.

### 4. Add focused regression tests

Add style tests covering:

- the default position is static;
- all four supported values parse and convert correctly;
- invalid values are rejected;
- removing position restores static;
- setting the current value remains a no-op and does not advance revisions.

Add reconciliation/layout tests covering:

1. static and relative nodes remain under their DOM parent;
2. an absolute node skips static ancestors and attaches to the nearest non-static ancestor;
3. an absolute node with no positioned ancestor attaches to the window;
4. a fixed node attaches to the window even inside a positioned ancestor;
5. an absolute or fixed node is removed from its original parent's normal flow;
6. positioned ancestors remain containing blocks for absolute descendants;
7. changing position rebuilds the effective topology without changing `dom.parent(node)`;
8. re-parented layout children retain deterministic DOM-preorder ordering.

Prefer assertions against `ComputedBox::layout_parent`, computed geometry, and the unchanged DOM parent. Existing event tests already cover DOM-based propagation and should not be duplicated unless this change breaks them.

## Validation

Run:

```sh
cargo fmt --all -- --check
cargo test -p burokku ui::elements
cargo test -p burokku ui::layout
cargo clippy -p burokku --all-targets -- -D warnings
```

## Known limitation

Burokku exposes the four inset longhands but not the `inset` shorthand. Nested
absolute or fixed boxes whose insets are all automatic still use Taffy's
flattened static-position approximation after re-parenting. Supporting fully
accurate CSS automatic static positions would require a placeholder or
post-layout anchor and should be driven by a concrete use case.

Stacking contexts currently cover the CSS triggers expressible by Burokku:
the window root, fixed boxes, positioned boxes with an integer `z-index`, and
static flex/grid items with an integer `z-index`. Other CSS triggers should be
added with the styles that introduce them.

## Non-goals

- mutating or duplicating the DOM tree;
- sticky positioning;
- stacking-context triggers for unsupported CSS properties;
- CSS containing-block triggers such as transforms and filters;
- adding a second layout tree implementation;
- changing event propagation, text inheritance, or scene paint ordering.
