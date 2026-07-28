use std::collections::HashMap;

use render::{TextConstraints, TextRunMetrics, TextSpan, TextStyle, TextSystem};
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout,
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
    elements::{
        styles::{Position, Style as ElementStyle},
        Document, ElementKind,
    },
    layouts::ScrollOffset,
};

use super::{
    style::to_taffy_style,
    text::{merge_text_style, normalize_text_spans, normalize_white_space},
};

pub(super) fn add_element(
    nodes: &mut Vec<LayoutNode>,
    document: &Document,
    element_id: u64,
    inherited_text_style: &TextStyle,
) -> usize {
    let element = document
        .node(element_id)
        .expect("element child IDs are validated when inserted");
    let text_style = merge_text_style(inherited_text_style, &element.style);
    let inline_spans = if matches!(element.kind, ElementKind::Span)
        && matches!(element.style.display, Display::Block)
        && should_collect_inline_spans(document, &element.children)
    {
        collect_inline_spans(document, element_id, &text_style)
    } else {
        None
    };
    let node_id = nodes.len();
    nodes.push(LayoutNode {
        element_id,
        kind: element.kind.clone(),
        style: to_taffy_style(&element.kind, &element.style),
        paint_style: element.style.clone(),
        text_style: text_style.clone(),
        inline_spans: inline_spans.clone(),
        rendered_spans: Vec::new(),
        children: Vec::with_capacity(element.children.len()),
        layout_children: Vec::with_capacity(element.children.len()),
        positioning_containing_block: None,
        static_offset: Point::ZERO,
        cache: Cache::new(),
        layout: TaffyLayout::new(),
        first_baseline: None,
        text_line_count: 0,
        text_runs: Vec::new(),
    });

    let children = if inline_spans.is_some() {
        Vec::new()
    } else {
        element
            .children
            .iter()
            .map(|child| add_element(nodes, document, *child, &text_style))
            .collect::<Vec<_>>()
    };
    let mut children = children;
    if matches!(element.style.display, Display::Flex | Display::Grid) {
        children.sort_by_key(|child| nodes[*child].paint_style.order);
    }
    nodes[node_id].children = children.clone();
    nodes[node_id].layout_children = children;
    node_id
}

/// Reparents out-of-flow boxes in the layout tree without changing the
/// retained DOM/paint tree.
///
/// Taffy lays absolute boxes out against their direct layout parent. CSS
/// instead uses the nearest positioned ancestor for absolute boxes and the
/// viewport or nearest transformed ancestor for fixed boxes.
pub(super) fn establish_positioning_containing_blocks(nodes: &mut [LayoutNode], root: usize) {
    let mut absolute_locations = vec![Point::ZERO; nodes.len()];
    collect_absolute_locations(nodes, root, Point::ZERO, &mut absolute_locations);

    for node in nodes.iter_mut() {
        node.layout_children.clear();
        node.positioning_containing_block = None;
        node.static_offset = Point::ZERO;
    }
    rebuild_layout_children(nodes, root, root, root, false);

    for node in 0..nodes.len() {
        let Some(owner) = nodes[node].positioning_containing_block else {
            continue;
        };
        nodes[node].static_offset = Point {
            x: absolute_locations[node].x - absolute_locations[owner].x,
            y: absolute_locations[node].y - absolute_locations[owner].y,
        };
    }
}

fn collect_absolute_locations(
    nodes: &[LayoutNode],
    node: usize,
    parent_location: Point<f32>,
    locations: &mut [Point<f32>],
) {
    let location = Point {
        x: parent_location.x + nodes[node].layout.location.x,
        y: parent_location.y + nodes[node].layout.location.y,
    };
    locations[node] = location;
    for child in &nodes[node].children {
        collect_absolute_locations(nodes, *child, location, locations);
    }
}

fn rebuild_layout_children(
    nodes: &mut [LayoutNode],
    node: usize,
    absolute_containing_block: usize,
    fixed_containing_block: usize,
    ancestor_hidden: bool,
) {
    let children = nodes[node].children.clone();
    let descendants_hidden = ancestor_hidden || nodes[node].style.display == Display::None;
    let establishes_absolute_containing_block = node == absolute_containing_block
        || nodes[node].paint_style.position.is_positioned()
        || !nodes[node].paint_style.transform.is_none();
    let descendant_absolute_containing_block = if establishes_absolute_containing_block {
        node
    } else {
        absolute_containing_block
    };
    let descendant_fixed_containing_block = if !nodes[node].paint_style.transform.is_none() {
        node
    } else {
        fixed_containing_block
    };

    for child in children {
        let out_of_flow_owner = if descendants_hidden {
            None
        } else {
            match nodes[child].paint_style.position {
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
            child,
            descendant_absolute_containing_block,
            descendant_fixed_containing_block,
            descendants_hidden,
        );
    }
}

fn should_collect_inline_spans(document: &Document, children: &[u64]) -> bool {
    let mut fragment_count = 0;
    let mut has_nested_span = false;
    for child_id in children {
        let Ok(child) = document.node(*child_id) else {
            continue;
        };
        match &child.kind {
            ElementKind::Text(text) if !text.is_empty() => fragment_count += 1,
            ElementKind::Span if child.style.display == Display::Block => {
                fragment_count += 1;
                has_nested_span = true;
            }
            _ => {}
        }
    }
    has_nested_span || fragment_count > 1
}

fn collect_inline_spans(
    document: &Document,
    element_id: u64,
    text_style: &TextStyle,
) -> Option<Vec<TextSpan>> {
    let element = document
        .node(element_id)
        .expect("inline span descendants are validated when inserted");
    let mut spans = Vec::new();
    for child_id in &element.children {
        let child = document
            .node(*child_id)
            .expect("inline span descendants are validated when inserted");
        match &child.kind {
            ElementKind::Text(text) => {
                if !text.is_empty() {
                    spans.push(TextSpan::new(text, text_style.clone()));
                }
            }
            ElementKind::Comment(_) => {}
            ElementKind::Span if child.style.display == Display::None => {}
            ElementKind::Span if child.style.display == Display::Block => {
                let child_style = merge_text_style(text_style, &child.style);
                spans.extend(collect_inline_spans(document, *child_id, &child_style)?);
            }
            _ => return None,
        }
    }
    (!spans.is_empty()).then_some(spans)
}

pub(super) struct LayoutNode {
    pub(super) element_id: u64,
    pub(super) kind: ElementKind,
    pub(super) style: TaffyStyle,
    pub(super) paint_style: ElementStyle,
    pub(super) text_style: TextStyle,
    pub(super) inline_spans: Option<Vec<TextSpan>>,
    pub(super) rendered_spans: Vec<TextSpan>,
    /// Children in DOM/order-modified tree order, used for painting.
    pub(super) children: Vec<usize>,
    /// Children participating directly in this node's Taffy layout.
    pub(super) layout_children: Vec<usize>,
    /// The CSS containing block used to lay out an absolute or fixed box.
    pub(super) positioning_containing_block: Option<usize>,
    /// Hypothetical in-flow position, relative to the positioning containing block.
    pub(super) static_offset: Point<f32>,
    cache: Cache,
    pub(super) layout: TaffyLayout,
    first_baseline: Option<f32>,
    pub(super) text_line_count: usize,
    pub(super) text_runs: Vec<TextRunMetrics>,
}

pub(super) struct ElementLayoutTree<'a> {
    pub(super) nodes: Vec<LayoutNode>,
    pub(super) text_system: &'a mut TextSystem,
    pub(super) scroll_offsets: &'a HashMap<u64, ScrollOffset>,
}

impl ElementLayoutTree<'_> {
    pub(super) fn clear_layout_caches(&mut self) {
        for node in &mut self.nodes {
            node.cache.clear();
            node.first_baseline = None;
        }
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
            let is_text = matches!(tree.nodes[index].kind, ElementKind::Text(_))
                || tree.nodes[index].inline_spans.is_some();
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
        let text_style = self.nodes[index].text_style.clone();
        let spans = match &self.nodes[index].inline_spans {
            Some(spans) => normalize_text_spans(spans, text_style.white_space),
            None => match &self.nodes[index].kind {
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

impl TraversePartialTree for ElementLayoutTree<'_> {
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

impl LayoutPartialTree for ElementLayoutTree<'_> {
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

impl CacheTree for ElementLayoutTree<'_> {
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

impl LayoutBlockContainer for ElementLayoutTree<'_> {
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

impl LayoutFlexboxContainer for ElementLayoutTree<'_> {
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

impl LayoutGridContainer for ElementLayoutTree<'_> {
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
