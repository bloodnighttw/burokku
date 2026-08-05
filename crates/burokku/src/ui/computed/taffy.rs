//! Low-level Taffy integration for [`Elements`].
//!
//! [`Elements`] owns its descendants recursively. Taffy's layout algorithms,
//! on the other hand, need stable node IDs and mutable per-node layout state.
//! [`LayoutTree`] bridges those models with a flat sidecar: it borrows the
//! elements, while layout results and caches live in a `Vec` indexed by
//! [`NodeId`]. This keeps the implementation safe and also permits full-tree
//! operations such as pixel rounding.

use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout, round_layout, AvailableSpace,
    BlockContext, Cache, CacheTree, Display, Layout, LayoutBlockContainer, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RoundTree, RunMode,
    Size, Style, TraversePartialTree, TraverseTree,
};

use crate::ui::elements::{accepts_child, Elements};

/// Supplies intrinsic sizes for leaf elements.
///
/// The callback may use the string stored by [`Elements::_String`], a text
/// system, image metadata, or any other application data. Containers without
/// valid children are also measured as leaves, matching Taffy's high-level
/// tree behavior.
pub trait MeasureElement {
    fn measure(
        &mut self,
        element: &Elements,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
    ) -> Size<f32>;
}

impl<F> MeasureElement for F
where
    F: FnMut(&Elements, Size<Option<f32>>, Size<AvailableSpace>) -> Size<f32>,
{
    fn measure(
        &mut self,
        element: &Elements,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
    ) -> Size<f32> {
        self(element, known_dimensions, available_space)
    }
}

/// A leaf measurer that gives unconstrained axes a size of zero.
#[derive(Clone, Copy, Debug, Default)]
pub struct ZeroMeasure;

impl MeasureElement for ZeroMeasure {
    fn measure(
        &mut self,
        _element: &Elements,
        known_dimensions: Size<Option<f32>>,
        _available_space: Size<AvailableSpace>,
    ) -> Size<f32> {
        Size {
            width: known_dimensions.width.unwrap_or(0.0),
            height: known_dimensions.height.unwrap_or(0.0),
        }
    }
}

struct LayoutNode<'elements> {
    element: &'elements Elements,
    style: Style<String>,
    children: Vec<NodeId>,
    cache: Cache,
    unrounded_layout: Layout,
    final_layout: Layout,
}

impl<'elements> LayoutNode<'elements> {
    fn new(element: &'elements Elements) -> Self {
        Self {
            element,
            style: style_for(element),
            children: Vec::new(),
            cache: Cache::new(),
            unrounded_layout: Layout::with_order(0),
            final_layout: Layout::with_order(0),
        }
    }
}

/// Persistent layout state for one [`Elements`] tree.
///
/// Keep this value between calls to [`LayoutTree::compute_layout`] to reuse
/// Taffy's per-node caches. Because it immutably borrows the element tree, the
/// styles and topology cannot change while their cached results are alive.
pub struct LayoutTree<'elements, Measure = ZeroMeasure> {
    nodes: Vec<LayoutNode<'elements>>,
    root: NodeId,
    measure: Measure,
}

impl<'elements> LayoutTree<'elements, ZeroMeasure> {
    /// Creates layout state using zero intrinsic size for leaf elements.
    pub fn new(root: &'elements Elements) -> Self {
        Self::with_measure(root, ZeroMeasure)
    }
}

impl<'elements, Measure> LayoutTree<'elements, Measure>
where
    Measure: MeasureElement,
{
    /// Creates layout state with a custom intrinsic-size callback.
    pub fn with_measure(root: &'elements Elements, measure: Measure) -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            root: NodeId::from(0usize),
            measure,
        };
        tree.root = tree.add_element(root);
        tree
    }

    /// Computes and pixel-rounds the layout for the requested available space.
    ///
    /// Repeated calls reuse cached sizing and layout results when the inputs
    /// match. Call [`LayoutTree::clear_cache`] if external data used by the
    /// measurement callback changes.
    pub fn compute_layout(&mut self, available_space: Size<AvailableSpace>) {
        let root = self.root;
        compute_root_layout(self, root, available_space);
        round_layout(self, root);
    }

    /// The Taffy node ID corresponding to the root element.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Number of valid elements represented in this layout tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether this layout tree contains no elements.
    ///
    /// A constructed tree always contains its root, so this returns `false`.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Finds the node ID for an element in this tree.
    pub fn node_id(&self, element: &Elements) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|node| std::ptr::eq(node.element, element))
            .map(NodeId::from)
    }

    /// Returns the element represented by a node ID.
    pub fn element(&self, node: NodeId) -> Option<&'elements Elements> {
        self.nodes.get(node_index(node)).map(|node| node.element)
    }

    /// Returns the valid child node IDs in source order.
    pub fn children(&self, node: NodeId) -> Option<&[NodeId]> {
        self.nodes
            .get(node_index(node))
            .map(|node| node.children.as_slice())
    }

    /// Returns the rounded layout produced by the latest calculation.
    pub fn layout(&self, node: NodeId) -> Option<&Layout> {
        self.nodes
            .get(node_index(node))
            .map(|node| &node.final_layout)
    }

    /// Returns Taffy's unrounded layout produced by the latest calculation.
    pub fn unrounded_layout(&self, node: NodeId) -> Option<&Layout> {
        self.nodes
            .get(node_index(node))
            .map(|node| &node.unrounded_layout)
    }

    /// Clears every per-node Taffy cache entry.
    pub fn clear_cache(&mut self) {
        for node in &mut self.nodes {
            node.cache.clear();
        }
    }

    fn add_element(&mut self, element: &'elements Elements) -> NodeId {
        let node_id = NodeId::from(self.nodes.len());
        self.nodes.push(LayoutNode::new(element));

        if let Some(children) = element.children() {
            for child in children
                .iter()
                .filter(|child| accepts_child(element, child))
            {
                let child_id = self.add_element(child);
                self.nodes[node_index(node_id)].children.push(child_id);
            }
        }

        node_id
    }

    fn compute_node_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node_id);
        }

        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let index = node_index(node_id);
            let display = tree.nodes[index].style.display;
            let has_children = !tree.nodes[index].children.is_empty();

            match (display, has_children) {
                (Display::None, _) => compute_hidden_layout(tree, node_id),
                (Display::Block, true) => {
                    compute_block_layout(tree, node_id, inputs, block_context)
                }
                (Display::Flex, true) => compute_flexbox_layout(tree, node_id, inputs),
                (Display::Grid, true) => compute_grid_layout(tree, node_id, inputs),
                (_, false) => {
                    let LayoutTree { nodes, measure, .. } = tree;
                    let node = &nodes[index];
                    compute_leaf_layout(
                        inputs,
                        &node.style,
                        |_value, _basis| 0.0,
                        |known_dimensions, available_space| {
                            measure.measure(node.element, known_dimensions, available_space)
                        },
                    )
                }
            }
        })
    }
}

/// Creates a layout tree and immediately calculates it with zero-size leaves.
pub fn compute_layout(root: &Elements, available_space: Size<AvailableSpace>) -> LayoutTree<'_> {
    let mut tree = LayoutTree::new(root);
    tree.compute_layout(available_space);
    tree
}

/// Creates a layout tree and immediately calculates it with a leaf measurer.
pub fn compute_layout_with_measure<Measure>(
    root: &Elements,
    available_space: Size<AvailableSpace>,
    measure: Measure,
) -> LayoutTree<'_, Measure>
where
    Measure: MeasureElement,
{
    let mut tree = LayoutTree::with_measure(root, measure);
    tree.compute_layout(available_space);
    tree
}

fn node_index(node: NodeId) -> usize {
    usize::from(node)
}

fn style_for(element: &Elements) -> Style<String> {
    match element {
        Elements::Flex { style, .. } => Style {
            display: Display::Flex,
            flex_direction: style.direction,
            flex_wrap: style.wrap,
            gap: style.gap,
            align_content: style.align_content,
            align_items: style.align_items,
            justify_content: style.justify_content,
            flex_basis: style.basis,
            flex_grow: style.grow,
            flex_shrink: style.shrink,
            align_self: style.align_self,
            ..Style::default()
        },
        Elements::Grid { style, .. } => Style {
            display: Display::Grid,
            grid_template_rows: style.template_rows.clone(),
            grid_template_columns: style.template_columns.clone(),
            grid_template_areas: style.template_areas.clone(),
            grid_template_row_names: style.template_row_names.clone(),
            grid_template_column_names: style.template_column_names.clone(),
            grid_auto_rows: style.auto_rows.clone(),
            grid_auto_columns: style.auto_columns.clone(),
            grid_auto_flow: style.auto_flow,
            gap: style.gap,
            align_content: style.align_content,
            justify_content: style.justify_content,
            align_items: style.align_items,
            justify_items: style.justify_items,
            grid_row: style.row.clone(),
            grid_column: style.column.clone(),
            align_self: style.align_self,
            justify_self: style.justify_self,
            ..Style::default()
        },
        Elements::App { .. }
        | Elements::Window { .. }
        | Elements::Div { .. }
        | Elements::Text { .. }
        | Elements::_String { .. } => Style::default(),
    }
}

impl<Measure> TraversePartialTree for LayoutTree<'_, Measure> {
    type ChildIter<'a>
        = std::iter::Copied<std::slice::Iter<'a, NodeId>>
    where
        Self: 'a;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        self.nodes[node_index(parent_node_id)]
            .children
            .iter()
            .copied()
    }

    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.nodes[node_index(parent_node_id)].children.len()
    }

    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        self.nodes[node_index(parent_node_id)].children[child_index]
    }
}

impl<Measure> TraverseTree for LayoutTree<'_, Measure> {}

impl<Measure> LayoutPartialTree for LayoutTree<'_, Measure>
where
    Measure: MeasureElement,
{
    type CoreContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.nodes[node_index(node_id)].style
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.nodes[node_index(node_id)].unrounded_layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_node_layout(node_id, inputs, None)
    }
}

impl<Measure> CacheTree for LayoutTree<'_, Measure> {
    fn cache_get(&self, node_id: NodeId, inputs: &LayoutInput) -> Option<LayoutOutput> {
        self.nodes[node_index(node_id)].cache.get(inputs)
    }

    fn cache_store(&mut self, node_id: NodeId, inputs: &LayoutInput, layout_output: LayoutOutput) {
        self.nodes[node_index(node_id)]
            .cache
            .store(inputs, layout_output);
    }

    fn cache_clear(&mut self, node_id: NodeId) {
        self.nodes[node_index(node_id)].cache.clear();
    }
}

impl<Measure> LayoutBlockContainer for LayoutTree<'_, Measure>
where
    Measure: MeasureElement,
{
    type BlockContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        &self.nodes[node_index(node_id)].style
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        &self.nodes[node_index(child_node_id)].style
    }

    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_node_layout(node_id, inputs, block_context)
    }
}

impl<Measure> LayoutFlexboxContainer for LayoutTree<'_, Measure>
where
    Measure: MeasureElement,
{
    type FlexboxContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        &self.nodes[node_index(node_id)].style
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        &self.nodes[node_index(child_node_id)].style
    }
}

impl<Measure> LayoutGridContainer for LayoutTree<'_, Measure>
where
    Measure: MeasureElement,
{
    type GridContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        &self.nodes[node_index(node_id)].style
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        &self.nodes[node_index(child_node_id)].style
    }
}

impl<Measure> RoundTree for LayoutTree<'_, Measure> {
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        self.nodes[node_index(node_id)].unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.nodes[node_index(node_id)].final_layout = *layout;
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use taffy::{geometry::Point, prelude::TaffyMaxContent, style_helpers::length, FlexDirection};

    use super::*;
    use crate::ui::elements::styles::{flex::FlexStyle, grid::GridStyle};

    fn text(value: &str) -> Elements {
        Elements::Text {
            children: vec![Elements::_String {
                string: value.into(),
            }],
        }
    }

    #[test]
    fn lays_out_valid_flex_children_and_rounds_results() {
        let root = Elements::Flex {
            style: Box::new(FlexStyle {
                direction: FlexDirection::Row,
                ..FlexStyle::default()
            }),
            children: vec![text("one"), text("two")],
        };

        let mut tree = LayoutTree::with_measure(&root, |element: &Elements, _, _| {
            let Elements::_String { string } = element else {
                return Size::ZERO;
            };
            Size {
                width: string.len() as f32 * 4.5,
                height: 7.5,
            }
        });
        tree.compute_layout(Size::MAX_CONTENT);

        let children = tree.children(tree.root()).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(tree.layout(children[0]).unwrap().location, Point::ZERO);
        assert_eq!(
            tree.layout(children[1]).unwrap().location,
            Point { x: 14.0, y: 0.0 }
        );
        assert_eq!(
            tree.layout(tree.root()).unwrap().size,
            Size {
                width: 27.0,
                height: 8.0
            }
        );
    }

    #[test]
    fn skips_children_that_are_invalid_for_their_parent() {
        let root = Elements::App {
            children: vec![
                Elements::Div {
                    children: vec![Elements::Window { children: vec![] }],
                },
                Elements::Window {
                    children: vec![Elements::Div { children: vec![] }],
                },
            ],
        };

        let tree = LayoutTree::new(&root);

        assert_eq!(tree.len(), 3);
        assert_eq!(tree.children(tree.root()).unwrap().len(), 1);
    }

    #[test]
    fn maps_grid_tracks_into_taffy_style() {
        let root = Elements::Grid {
            style: Box::new(GridStyle {
                template_columns: vec![length(20.0_f32), length(30.0_f32)],
                ..GridStyle::default()
            }),
            children: vec![text("one"), text("two")],
        };

        let tree = compute_layout(&root, Size::MAX_CONTENT);
        let children = tree.children(tree.root()).unwrap();

        assert_eq!(tree.layout(children[0]).unwrap().location, Point::ZERO);
        assert_eq!(
            tree.layout(children[1]).unwrap().location,
            Point { x: 20.0, y: 0.0 }
        );
        assert_eq!(tree.layout(tree.root()).unwrap().size.width, 50.0);
    }

    #[test]
    fn reuses_cache_until_it_is_explicitly_cleared() {
        let measurements = Rc::new(Cell::new(0));
        let callback_measurements = measurements.clone();
        let root = Elements::Flex {
            style: Box::new(FlexStyle::default()),
            children: vec![text("cached")],
        };
        let mut tree = LayoutTree::with_measure(&root, move |_: &Elements, _, _| {
            callback_measurements.set(callback_measurements.get() + 1);
            Size {
                width: 20.0,
                height: 10.0,
            }
        });

        tree.compute_layout(Size::MAX_CONTENT);
        let first_run = measurements.get();
        assert!(first_run > 0);

        tree.compute_layout(Size::MAX_CONTENT);
        assert_eq!(measurements.get(), first_run);

        tree.clear_cache();
        tree.compute_layout(Size::MAX_CONTENT);
        assert!(measurements.get() > first_run);
    }
}
