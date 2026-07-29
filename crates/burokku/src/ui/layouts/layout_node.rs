use std::collections::HashMap;

use render::{TextConstraints, TextRunMetrics, TextSpan, TextSystem};
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout,
    geometry::{Point, Size},
    prelude::{AvailableSpace, Display, NodeId},
    style::Style as TaffyStyle,
    tree::{
        Cache, Layout as TaffyLayout, LayoutBlockContainer, LayoutFlexboxContainer,
        LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, RunMode,
        TraversePartialTree,
    },
    BlockContext, CacheTree,
};

use crate::ui::{
    elements::{styles::Position, ElementKind},
    layouts::{compute::text, render_node::RenderNode, ScrollOffset},
};

use super::compute::style::to_taffy_style;
use text::{normalize_text_spans, normalize_white_space};

fn add_render_node<'render>(
    nodes: &mut Vec<TaffyNode>,
    render_nodes: &mut Vec<&'render RenderNode>,
    render_node: &'render RenderNode,
) -> usize {
    let node_id = nodes.len();
    render_nodes.push(render_node);
    nodes.push(TaffyNode {
        style: to_taffy_style(&render_node.kind, &render_node.style),
        rendered_spans: Vec::new(),
        render_children: Vec::with_capacity(render_node.paint_children.len()),
        layout_children: Vec::with_capacity(render_node.paint_children.len()),
        positioning_containing_block: None,
        static_offset: Point::ZERO,
        cache: Cache::new(),
        layout: TaffyLayout::new(),
        first_baseline: None,
        text_line_count: 0,
        text_runs: Vec::new(),
    });

    let children = render_node
        .paint_children
        .iter()
        .map(|child| add_render_node(nodes, render_nodes, child))
        .collect::<Vec<_>>();
    nodes[node_id].render_children = children.clone();
    nodes[node_id].layout_children = children;
    node_id
}

/// Reparents out-of-flow boxes while lowering the stable render tree.
///
/// Taffy resolves absolute positioning against a node's direct layout parent.
/// CSS instead selects a containing block that may be higher in the render
/// tree, so absolute and fixed nodes must be attached to that block here.
fn reparent_out_of_flow_nodes(nodes: &mut [TaffyNode], render_nodes: &[&RenderNode], root: usize) {
    for node in nodes.iter_mut() {
        node.layout_children.clear();
        node.positioning_containing_block = None;
    }
    rebuild_layout_children(nodes, render_nodes, root, root, root, false);
}

fn rebuild_layout_children(
    nodes: &mut [TaffyNode],
    render_nodes: &[&RenderNode],
    node: usize,
    absolute_containing_block: usize,
    fixed_containing_block: usize,
    ancestor_hidden: bool,
) {
    let children = nodes[node].render_children.clone();
    let descendants_hidden = ancestor_hidden || nodes[node].style.display == Display::None;
    let establishes_absolute_containing_block = node == absolute_containing_block
        || render_nodes[node].style.position.is_positioned()
        || !render_nodes[node].style.transform.is_none();
    let descendant_absolute_containing_block = if establishes_absolute_containing_block {
        node
    } else {
        absolute_containing_block
    };
    let descendant_fixed_containing_block = if !render_nodes[node].style.transform.is_none() {
        node
    } else {
        fixed_containing_block
    };

    for child in children {
        let out_of_flow_owner = if descendants_hidden {
            None
        } else {
            match render_nodes[child].style.position {
                Position::Absolute => Some(descendant_absolute_containing_block),
                Position::Fixed => Some(descendant_fixed_containing_block),
                Position::Static | Position::Relative => None,
            }
        };
        let layout_parent = out_of_flow_owner.unwrap_or(node);
        if let Some(owner) = out_of_flow_owner {
            nodes[child].positioning_containing_block = Some(owner);
        }
        nodes[layout_parent].layout_children.push(child);

        rebuild_layout_children(
            nodes,
            render_nodes,
            child,
            descendant_absolute_containing_block,
            descendant_fixed_containing_block,
            descendants_hidden,
        );
    }
}

pub(super) struct TaffyNode {
    pub(super) style: TaffyStyle,
    pub(super) rendered_spans: Vec<TextSpan>,
    /// IDs matching children in the stable render tree.
    pub(super) render_children: Vec<usize>,
    /// Children participating directly in this node's Taffy layout.
    pub(super) layout_children: Vec<usize>,
    /// The CSS containing block used to lay out an absolute or fixed box.
    pub(super) positioning_containing_block: Option<usize>,
    /// Hypothetical render-tree location relative to the containing block.
    pub(super) static_offset: Point<f32>,
    cache: Cache,
    pub(super) layout: TaffyLayout,
    first_baseline: Option<f32>,
    pub(super) text_line_count: usize,
    pub(super) text_runs: Vec<TextRunMetrics>,
}

pub(super) struct LayoutNode<'render, 'state> {
    pub(super) nodes: Vec<TaffyNode>,
    pub(super) render_nodes: Vec<&'render RenderNode>,
    pub(super) viewport_root: usize,
    pub(super) text_system: &'state mut TextSystem,
    pub(super) scroll_offsets: &'state HashMap<u64, ScrollOffset>,
}

impl<'render, 'state> LayoutNode<'render, 'state> {
    /// Lowers the paint tree into the Taffy tree.
    ///
    /// This is the only render-to-layout transition. It assigns stable node
    /// IDs and reparents absolute/fixed nodes before Taffy sees the tree.
    pub(super) fn from_render_node(
        render_root: &'render RenderNode,
        text_system: &'state mut TextSystem,
        scroll_offsets: &'state HashMap<u64, ScrollOffset>,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut render_nodes = Vec::new();
        let viewport_root = add_render_node(&mut nodes, &mut render_nodes, render_root);
        debug_assert_eq!(viewport_root, 0);
        reparent_out_of_flow_nodes(&mut nodes, &render_nodes, viewport_root);
        Self {
            nodes,
            render_nodes,
            viewport_root,
            text_system,
            scroll_offsets,
        }
    }

    /// Builds the structural probe used only to recover CSS static positions
    /// for hoisted boxes whose insets are `auto`.
    pub(super) fn static_position_probe(
        render_root: &'render RenderNode,
        text_system: &'state mut TextSystem,
        scroll_offsets: &'state HashMap<u64, ScrollOffset>,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut render_nodes = Vec::new();
        let viewport_root = add_render_node(&mut nodes, &mut render_nodes, render_root);
        debug_assert_eq!(viewport_root, 0);
        Self {
            nodes,
            render_nodes,
            viewport_root,
            text_system,
            scroll_offsets,
        }
    }

    pub(super) fn absolute_render_locations(&self) -> Vec<Point<f32>> {
        let mut locations = vec![Point::ZERO; self.nodes.len()];
        self.collect_absolute_render_locations(self.viewport_root, Point::ZERO, &mut locations);
        locations
    }

    fn collect_absolute_render_locations(
        &self,
        node: usize,
        parent_location: Point<f32>,
        locations: &mut [Point<f32>],
    ) {
        let location = Point {
            x: parent_location.x + self.nodes[node].layout.location.x,
            y: parent_location.y + self.nodes[node].layout.location.y,
        };
        locations[node] = location;
        for child in &self.nodes[node].render_children {
            self.collect_absolute_render_locations(*child, location, locations);
        }
    }

    pub(super) fn set_static_offsets(&mut self, absolute_locations: &[Point<f32>]) {
        for (node, data) in self.nodes.iter_mut().enumerate() {
            let Some(owner) = data.positioning_containing_block else {
                continue;
            };
            data.static_offset = Point {
                x: absolute_locations[node].x - absolute_locations[owner].x,
                y: absolute_locations[node].y - absolute_locations[owner].y,
            };
        }
    }

    pub(super) fn compute_layout(&mut self, available_space: Size<AvailableSpace>) {
        compute_root_layout(self, NodeId::from(self.viewport_root), available_space);
    }

    fn compute_node(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node_id);
        }

        let output = compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let index = usize::from(node_id);
            let display = tree.nodes[index].style.display;
            let is_text = tree.render_nodes[index].is_text_flow();
            let has_children = !tree.nodes[index].layout_children.is_empty();

            match (display, is_text, has_children) {
                (Display::None, _, _) => compute_hidden_layout(tree, node_id),
                (_, true, _) => tree.compute_text_layout(node_id, inputs),
                (Display::Block, false, true) => {
                    let mut output = compute_block_layout(tree, node_id, inputs, block_context);
                    output.first_baselines.y = tree.nodes[index]
                        .layout_children
                        .iter()
                        .filter(|child| {
                            tree.nodes[**child].style.display != Display::None
                                && tree.nodes[**child].style.position
                                    != taffy::style::Position::Absolute
                        })
                        .find_map(|child| {
                            tree.nodes[*child]
                                .first_baseline
                                .map(|baseline| tree.nodes[*child].layout.location.y + baseline)
                        });
                    output
                }
                (Display::Flex, false, true) => compute_flexbox_layout(tree, node_id, inputs),
                (Display::Grid, false, true) => compute_grid_layout(tree, node_id, inputs),
                (_, false, false) => {
                    let style = &tree.nodes[index].style;
                    compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known, _| Size {
                            width: known.width.unwrap_or(0.0),
                            height: known.height.unwrap_or(0.0),
                        },
                    )
                }
            }
        });
        self.nodes[usize::from(node_id)].first_baseline = output.first_baselines.y;
        output
    }

    fn compute_text_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let index = usize::from(node_id);
        let style = self.nodes[index].style.clone();
        let render_node = self.render_nodes[index];
        let text_style = render_node.text_style.clone();
        let spans = match &render_node.inline_spans {
            Some(spans) => normalize_text_spans(spans, text_style.white_space),
            None => match &render_node.kind {
                ElementKind::Text(text) => vec![TextSpan::new(
                    normalize_white_space(text, text_style.white_space),
                    text_style.clone(),
                )],
                _ => unreachable!("only text flows use text measurement"),
            },
        };
        self.nodes[index].rendered_spans = spans.clone();

        let mut first_baseline = None;
        let mut output = compute_leaf_layout(
            inputs,
            &style,
            |_, _| 0.0,
            |known_dimensions, available_space| {
                let constraints = if let Some(width) = known_dimensions.width {
                    TextConstraints::at_most(width)
                } else {
                    match available_space.width {
                        AvailableSpace::Definite(width) => TextConstraints::at_most(width),
                        AvailableSpace::MinContent => TextConstraints::MIN_CONTENT,
                        AvailableSpace::MaxContent => TextConstraints::UNCONSTRAINED,
                    }
                };
                let measured =
                    self.text_system
                        .layout_rich_metrics(&spans, &text_style, constraints);
                first_baseline = Some(measured.text.first_baseline);
                self.nodes[index].text_line_count = measured.text.line_count;
                self.nodes[index].text_runs = measured.runs;
                Size {
                    width: known_dimensions.width.unwrap_or(measured.text.width),
                    height: known_dimensions.height.unwrap_or(measured.text.height),
                }
            },
        );
        output.first_baselines.y = first_baseline;
        output
    }
}

pub(super) struct ChildIter<'a>(std::slice::Iter<'a, usize>);

impl Iterator for ChildIter<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied().map(NodeId::from)
    }
}

impl TraversePartialTree for LayoutNode<'_, '_> {
    type ChildIter<'a>
        = ChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, node_id: NodeId) -> Self::ChildIter<'_> {
        ChildIter(self.nodes[usize::from(node_id)].layout_children.iter())
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.nodes[usize::from(node_id)].layout_children.len()
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        NodeId::from(self.nodes[usize::from(node_id)].layout_children[index])
    }
}

impl LayoutPartialTree for LayoutNode<'_, '_> {
    type CustomIdent = String;
    type CoreContainerStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;

    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.nodes[usize::from(node_id)].style
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &TaffyLayout) {
        self.nodes[usize::from(node_id)].layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_node(node_id, inputs, None)
    }
}

impl CacheTree for LayoutNode<'_, '_> {
    fn cache_get(&self, node_id: NodeId, inputs: &LayoutInput) -> Option<LayoutOutput> {
        // A cached final container layout does not restore the final layouts
        // of its descendants. Intrinsic sizing can overwrite a text child's
        // geometry after that entry was stored, leaving the parent at its
        // final size but the text shaped and positioned for min-content.
        // Re-run final layout passes so the retained descendant geometry
        // always matches the container result that will be painted.
        if inputs.run_mode == RunMode::PerformLayout {
            return None;
        }
        self.nodes[usize::from(node_id)].cache.get(inputs)
    }

    fn cache_store(&mut self, node_id: NodeId, inputs: &LayoutInput, output: LayoutOutput) {
        self.nodes[usize::from(node_id)].cache.store(inputs, output);
    }

    fn cache_clear(&mut self, node_id: NodeId) {
        self.nodes[usize::from(node_id)].cache.clear();
    }
}

impl LayoutBlockContainer for LayoutNode<'_, '_> {
    type BlockContainerStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_block_child_style(&self, node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_node(node_id, inputs, block_context)
    }
}

impl LayoutFlexboxContainer for LayoutNode<'_, '_> {
    type FlexboxContainerStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_flexbox_child_style(&self, node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.get_core_container_style(node_id)
    }
}

impl LayoutGridContainer for LayoutNode<'_, '_> {
    type GridContainerStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a TaffyStyle
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.get_core_container_style(node_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use render::TextSystem;

    use crate::ui::{
        elements::{Document, ElementKind, BODY_ID},
        layouts::ScrollOffset,
    };

    use super::LayoutNode;
    use crate::ui::layouts::render_node::RenderNode;

    #[test]
    fn absolute_layout_node_is_reparented_without_changing_render_parent() {
        let mut document = Document::new();
        let containing_block = document.create_node(ElementKind::Div);
        let wrapper = document.create_node(ElementKind::Div);
        let absolute = document.create_node(ElementKind::Div);
        document
            .set_style(containing_block, "position", Some("relative"))
            .unwrap();
        document
            .set_style(absolute, "position", Some("absolute"))
            .unwrap();
        document.insert(BODY_ID, containing_block, None).unwrap();
        document.insert(containing_block, wrapper, None).unwrap();
        document.insert(wrapper, absolute, None).unwrap();

        let render_root = RenderNode::viewport(RenderNode::from_document(&document));
        let mut text_system = TextSystem::new();
        let scroll_offsets = HashMap::<u64, ScrollOffset>::new();
        let tree = LayoutNode::from_render_node(&render_root, &mut text_system, &scroll_offsets);
        let root = tree.viewport_root;
        let body = tree.nodes[root].render_children[0];
        let containing_block_node = tree.nodes[body].render_children[0];
        let wrapper_node = tree.nodes[containing_block_node].render_children[0];
        let absolute_node = tree.nodes[wrapper_node].render_children[0];

        assert_eq!(tree.nodes[wrapper_node].render_children, [absolute_node]);
        assert!(tree.nodes[wrapper_node].layout_children.is_empty());
        assert_eq!(
            tree.nodes[containing_block_node].layout_children,
            [wrapper_node, absolute_node]
        );
        assert_eq!(
            tree.nodes[absolute_node].positioning_containing_block,
            Some(containing_block_node)
        );
    }

    #[test]
    fn fixed_layout_node_is_reparented_to_transformed_ancestor() {
        let mut document = Document::new();
        let containing_block = document.create_node(ElementKind::Div);
        let wrapper = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        document
            .set_style(containing_block, "transform", Some("translateX(0px)"))
            .unwrap();
        document
            .set_style(fixed, "position", Some("fixed"))
            .unwrap();
        document.insert(BODY_ID, containing_block, None).unwrap();
        document.insert(containing_block, wrapper, None).unwrap();
        document.insert(wrapper, fixed, None).unwrap();

        let render_root = RenderNode::viewport(RenderNode::from_document(&document));
        let mut text_system = TextSystem::new();
        let scroll_offsets = HashMap::<u64, ScrollOffset>::new();
        let tree = LayoutNode::from_render_node(&render_root, &mut text_system, &scroll_offsets);
        let root = tree.viewport_root;
        let body = tree.nodes[root].render_children[0];
        let containing_block_node = tree.nodes[body].render_children[0];
        let wrapper_node = tree.nodes[containing_block_node].render_children[0];
        let fixed_node = tree.nodes[wrapper_node].render_children[0];

        assert_eq!(tree.nodes[wrapper_node].render_children, [fixed_node]);
        assert!(tree.nodes[wrapper_node].layout_children.is_empty());
        assert_eq!(
            tree.nodes[containing_block_node].layout_children,
            [wrapper_node, fixed_node]
        );
        assert_eq!(
            tree.nodes[fixed_node].positioning_containing_block,
            Some(containing_block_node)
        );
    }
}
