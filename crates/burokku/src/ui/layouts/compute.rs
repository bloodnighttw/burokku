mod paint;
mod scroll;
mod style;
mod text;

use paint::{
    anchored_transform, box_style, multiply_transform, relative_transform,
    relative_transform_matrix,
};
use scroll::{overflow_clip, padding_box, scroll_container, scroll_content_size, OffsetContext};
use style::to_taffy_style;
use text::{merge_text_style, normalize_text_spans, normalize_white_space};

use std::collections::HashMap;

use render::{
    Clip, Rect as RenderRect, TextConstraints, TextRunMetrics, TextSpan, TextStyle, TextSystem,
    Transform,
};
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout,
    geometry::{Point, Size},
    prelude::{AvailableSpace, Dimension, Display, NodeId},
    style::Style as TaffyStyle,
    tree::{
        Cache, Layout as TaffyLayout, LayoutBlockContainer, LayoutFlexboxContainer,
        LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, RunMode,
        TraversePartialTree,
    },
    BlockContext, CacheTree,
};

use crate::ui::elements::{
    styles::{Isolation, Overflow as ElementOverflow, Style as ElementStyle, ZIndex},
    Document, ElementKind, BODY_ID,
};

use super::{Layout, LayoutKind, ScrollContainer, ScrollOffset, Scrollbar, ScrollbarAxis};

/// Computes a renderable layout tree from an element document.
///
/// The viewport and all returned geometry are in logical CSS pixels. Text is
/// measured by [`TextSystem`], which uses the same Glyphon shaping engine as
/// the renderer.
pub(super) fn compute_layout(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
) -> Layout {
    compute_layout_with_scroll(
        document,
        viewport_width,
        viewport_height,
        text_system,
        &HashMap::new(),
    )
}

pub(super) fn compute_layout_with_scroll(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
    scroll_offsets: &HashMap<u64, ScrollOffset>,
) -> Layout {
    let viewport = Size {
        width: viewport_width.max(0.0),
        height: viewport_height.max(0.0),
    };
    let mut nodes = Vec::new();
    let root = add_element(&mut nodes, document, BODY_ID, &TextStyle::default());
    nodes[root].style.size = Size {
        width: Dimension::length(viewport.width),
        height: Dimension::length(viewport.height),
    };

    let mut tree = ElementLayoutTree {
        nodes,
        text_system,
        scroll_offsets,
    };
    compute_root_layout(
        &mut tree,
        NodeId::from(root),
        viewport.map(AvailableSpace::Definite),
    );
    tree.to_layout(
        root,
        Point::ZERO,
        &[],
        RenderRect::new(0.0, 0.0, viewport.width, viewport.height),
        Transform::IDENTITY,
        false,
    )
}

fn add_element(
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
    nodes[node_id].children = children;
    node_id
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

struct LayoutNode {
    element_id: u64,
    kind: ElementKind,
    style: TaffyStyle,
    paint_style: ElementStyle,
    text_style: TextStyle,
    inline_spans: Option<Vec<TextSpan>>,
    rendered_spans: Vec<TextSpan>,
    children: Vec<usize>,
    cache: Cache,
    layout: TaffyLayout,
    first_baseline: Option<f32>,
    text_line_count: usize,
    text_runs: Vec<TextRunMetrics>,
}

struct ElementLayoutTree<'a> {
    nodes: Vec<LayoutNode>,
    text_system: &'a mut TextSystem,
    scroll_offsets: &'a HashMap<u64, ScrollOffset>,
}

impl ElementLayoutTree<'_> {
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
            let has_children = !tree.nodes[index].children.is_empty();

            match (display, is_text, has_children) {
                (Display::None, _, _) => compute_hidden_layout(tree, node_id),
                (_, true, _) => tree.compute_text_layout(node_id, inputs),
                (Display::Block, false, true) => {
                    let mut output = compute_block_layout(tree, node_id, inputs, block_context);
                    output.first_baselines.y = tree.nodes[index]
                        .children
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

    fn to_layout(
        &self,
        node: usize,
        parent_location: Point<f32>,
        ancestor_clips: &[Clip],
        viewport: RenderRect,
        parent_transform: Transform,
        flex_or_grid_item: bool,
    ) -> Layout {
        let data = &self.nodes[node];
        let location = Point {
            x: parent_location.x + data.layout.location.x,
            y: parent_location.y + data.layout.location.y,
        };
        let width = data.layout.size.width;
        let height = data.layout.size.height;
        let center = [location.x + width * 0.5, location.y + height * 0.5];
        let world_transform = multiply_transform(
            parent_transform,
            anchored_transform(
                Transform {
                    matrix: data.paint_style.transform.matrix(),
                },
                center,
            ),
        );
        let relative_transform = relative_transform(world_transform, center);
        let mut descendant_clips = ancestor_clips.to_vec();
        let mut own_clip = overflow_clip(data, location, width, height, viewport);
        if let Some(clip) = &mut own_clip {
            let clip_center = [
                clip.rect.x + clip.rect.width * 0.5,
                clip.rect.y + clip.rect.height * 0.5,
            ];
            clip.transform = relative_transform_matrix(world_transform, clip_center);
        }
        if let Some(clip) = own_clip {
            descendant_clips.push(clip);
        }
        let is_text_flow = matches!(data.kind, ElementKind::Text(_)) || data.inline_spans.is_some();
        let (kind, scroll) = if is_text_flow {
            let mut style = data.text_style.clone();
            style.opacity = data.paint_style.opacity;
            style.transform = Transform {
                matrix: data.paint_style.transform.matrix(),
            };
            (
                LayoutKind::Text {
                    text: data
                        .rendered_spans
                        .iter()
                        .map(|span| span.text.as_str())
                        .collect(),
                    spans: data.rendered_spans.clone(),
                    style,
                    has_transform: !data.paint_style.transform.is_none(),
                    line_count: data.text_line_count,
                    runs: data.text_runs.clone(),
                },
                None,
            )
        } else {
            match &data.kind {
                ElementKind::Comment(_)
                | ElementKind::Button
                | ElementKind::Div
                | ElementKind::Heading(_)
                | ElementKind::Image
                | ElementKind::Select
                | ElementKind::Span
                | ElementKind::Body
                | ElementKind::Other(_) => {
                    let scrolls_x = matches!(
                        data.paint_style.overflow_x,
                        ElementOverflow::Auto | ElementOverflow::Scroll
                    );
                    let scrolls_y = matches!(
                        data.paint_style.overflow_y,
                        ElementOverflow::Auto | ElementOverflow::Scroll
                    );
                    let requested = self
                        .scroll_offsets
                        .get(&data.element_id)
                        .copied()
                        .unwrap_or(ScrollOffset::ZERO);
                    let mut offset = ScrollOffset::new(
                        if scrolls_x { requested.x.max(0.0) } else { 0.0 },
                        if scrolls_y { requested.y.max(0.0) } else { 0.0 },
                    );
                    let child_parent = Point {
                        x: location.x - offset.x,
                        y: location.y - offset.y,
                    };
                    let children_are_flex_or_grid_items =
                        matches!(data.paint_style.display, Display::Flex | Display::Grid);
                    let mut children: Vec<_> = data
                        .children
                        .iter()
                        .map(|child| {
                            self.to_layout(
                                *child,
                                child_parent,
                                &descendant_clips,
                                viewport,
                                world_transform,
                                children_are_flex_or_grid_items,
                            )
                        })
                        .collect();
                    let scroll_viewport = padding_box(data, location, width, height);
                    let (content_width, content_height) =
                        scroll_content_size(&children, scroll_viewport, offset);
                    let max_offset = ScrollOffset::new(
                        if scrolls_x {
                            (content_width - scroll_viewport.width).max(0.0)
                        } else {
                            0.0
                        },
                        if scrolls_y {
                            (content_height - scroll_viewport.height).max(0.0)
                        } else {
                            0.0
                        },
                    );
                    let clamped =
                        ScrollOffset::new(offset.x.min(max_offset.x), offset.y.min(max_offset.y));
                    if clamped != offset {
                        offset = clamped;
                        let child_parent = Point {
                            x: location.x - offset.x,
                            y: location.y - offset.y,
                        };
                        children = data
                            .children
                            .iter()
                            .map(|child| {
                                self.to_layout(
                                    *child,
                                    child_parent,
                                    &descendant_clips,
                                    viewport,
                                    world_transform,
                                    children_are_flex_or_grid_items,
                                )
                            })
                            .collect();
                    }
                    let scroll = (scrolls_x || scrolls_y).then(|| {
                        scroll_container(
                            scroll_viewport,
                            own_clip.expect("scroll containers establish an overflow clip"),
                            content_width,
                            content_height,
                            OffsetContext::new(offset, max_offset),
                            data.paint_style.overflow_x == ElementOverflow::Scroll,
                            data.paint_style.overflow_y == ElementOverflow::Scroll,
                        )
                    });
                    (
                        LayoutKind::Box {
                            style: box_style(
                                &data.paint_style,
                                width,
                                height,
                                data.paint_style.opacity,
                                Transform {
                                    matrix: data.paint_style.transform.matrix(),
                                },
                            ),
                            has_transform: !data.paint_style.transform.is_none(),
                            z_index: match data.paint_style.z_index {
                                ZIndex::Auto => None,
                                ZIndex::Value(index) => Some(index),
                            },
                            isolated: data.paint_style.isolation == Isolation::Isolate,
                            positioned: data.paint_style.positioned,
                            flex_or_grid_item,
                            children,
                        },
                        scroll,
                    )
                }
                ElementKind::Text(_) => unreachable!("text nodes are handled as text flows"),
            }
        };

        Layout {
            element_id: data.element_id,
            x: location.x,
            y: location.y,
            width,
            height,
            transform: relative_transform,
            clips: ancestor_clips.to_vec(),
            scroll,
            kind,
        }
    }
}

struct ChildIter<'a>(std::slice::Iter<'a, usize>);

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
        ChildIter(self.nodes[usize::from(node_id)].children.iter())
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.nodes[usize::from(node_id)].children.len()
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        NodeId::from(self.nodes[usize::from(node_id)].children[index])
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

#[cfg(test)]
mod tests {
    use super::*;
    use render::{Color, FontFamily, FontStyle, TextAlign, TextConstraints, TextWrap};

    #[test]
    fn computes_flex_geometry_and_glyphon_text_metrics() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Text("first".into()));
        let second = document.create_node(ElementKind::Text("second".into()));
        document.set_style(row, "display", Some("flex")).unwrap();
        document.set_style(row, "width", Some("300px")).unwrap();
        document.set_style(row, "padding", Some("10px")).unwrap();
        document.set_style(row, "gap", Some("20px")).unwrap();
        document
            .set_style(row, "background-color", Some("#102030"))
            .unwrap();
        document.set_style(row, "font-size", Some("20px")).unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, first, None).unwrap();
        document.insert(row, second, None).unwrap();

        let mut text_system = TextSystem::new();
        let layout = compute_layout(&document, 800.0, 600.0, &mut text_system);
        let LayoutKind::Box { children, .. } = &layout.kind else {
            panic!("body should produce a box layout");
        };
        let LayoutKind::Box {
            style,
            children: row_children,
            ..
        } = &children[0].kind
        else {
            panic!("div should produce a box layout");
        };

        assert_eq!((layout.width, layout.height), (800.0, 600.0));
        assert_eq!(children[0].width, 320.0);
        assert_eq!(children[0].x, 0.0);
        assert_eq!(row_children.len(), 2);
        assert_eq!(row_children[0].x, 10.0);
        assert!(row_children[0].width > 0.0);
        assert_eq!(row_children[1].x, 10.0 + row_children[0].width + 20.0);
        assert_eq!(style.background, Color::from_rgba8(0x10, 0x20, 0x30, 0xff));
        let LayoutKind::Text {
            style: text_style, ..
        } = &row_children[0].kind
        else {
            panic!("text should produce a text layout");
        };
        assert_eq!(text_style.font_size, 20.0);
    }

    #[test]
    fn nested_flex_items_retain_final_text_geometry() {
        let mut document = Document::new();
        let gallery = document.create_node(ElementKind::Div);
        let card = document.create_node(ElementKind::Div);
        let row = document.create_node(ElementKind::Div);
        let large_box = document.create_node(ElementKind::Span);
        let small_box = document.create_node(ElementKind::Span);
        let fixed_width_sibling = document.create_node(ElementKind::Span);
        let large = document.create_node(ElementKind::Text("Baseline".into()));
        let small =
            document.create_node(ElementKind::Text("aligned through Glyphon metrics".into()));
        let sibling = document.create_node(ElementKind::Text(
            "Styled, centered, spaced and decorated text with font fallbacks".into(),
        ));
        document
            .set_style(gallery, "display", Some("flex"))
            .unwrap();
        document
            .set_style(gallery, "flex-direction", Some("column"))
            .unwrap();
        document.set_style(gallery, "width", Some("666px")).unwrap();
        document.set_style(card, "display", Some("flex")).unwrap();
        document
            .set_style(card, "flex-direction", Some("column"))
            .unwrap();
        document.set_style(card, "padding", Some("16px")).unwrap();
        document.set_style(row, "display", Some("flex")).unwrap();
        document.set_style(row, "gap", Some("10px")).unwrap();
        document
            .set_style(large_box, "font-size", Some("30px"))
            .unwrap();
        document
            .set_style(small_box, "font-size", Some("14px"))
            .unwrap();
        document
            .set_style(fixed_width_sibling, "width", Some("610px"))
            .unwrap();
        document.insert(BODY_ID, gallery, None).unwrap();
        document.insert(gallery, card, None).unwrap();
        document.insert(card, row, None).unwrap();
        document.insert(card, fixed_width_sibling, None).unwrap();
        document.insert(row, large_box, None).unwrap();
        document.insert(row, small_box, None).unwrap();
        document.insert(large_box, large, None).unwrap();
        document.insert(small_box, small, None).unwrap();
        document.insert(fixed_width_sibling, sibling, None).unwrap();

        let layout = compute_layout(&document, 800.0, 600.0, &mut TextSystem::new());
        let items = layout.children()[0].children()[0].children()[0].children();
        let small_text = &items[1].children()[0];

        assert_eq!(small_text.width, items[1].width);
        assert!(small_text.height <= items[1].height);
    }

    #[test]
    fn recomputes_normal_line_height_and_inherits_typography() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Text("styled text".into()));
        document
            .set_style(parent, "font-size", Some("30px"))
            .unwrap();
        document
            .set_style(parent, "font-family", Some("Missing Face, serif"))
            .unwrap();
        document
            .set_style(parent, "font-style", Some("oblique"))
            .unwrap();
        document
            .set_style(parent, "text-align", Some("right"))
            .unwrap();
        document
            .set_style(parent, "letter-spacing", Some("2px"))
            .unwrap();
        document
            .set_style(parent, "word-spacing", Some("4px"))
            .unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let text = &layout.children()[0].children()[0];
        let LayoutKind::Text { style, .. } = &text.kind else {
            panic!("child should be text");
        };

        assert_eq!(style.font_size, 30.0);
        assert_eq!(style.line_height, 36.0);
        assert!(style.line_height_is_normal);
        assert_eq!(
            style.font_families,
            vec![
                FontFamily::Named("Missing Face".to_owned()),
                FontFamily::Serif
            ]
        );
        assert_eq!(style.font_style, FontStyle::Oblique);
        assert_eq!(style.text_align, TextAlign::Right);
        assert_eq!(style.letter_spacing, 2.0);
        assert_eq!(style.word_spacing, 4.0);
    }

    #[test]
    fn nested_spans_share_inline_layout_and_keep_individual_styles() {
        let mut document = Document::new();
        let line = document.create_node(ElementKind::Span);
        let leading = document.create_node(ElementKind::Text("Hello  ".into()));
        let emphasized = document.create_node(ElementKind::Span);
        let emphasized_text = document.create_node(ElementKind::Text("world".into()));
        let reactive = document.create_node(ElementKind::Text(" 1".into()));
        document.set_style(line, "width", Some("70px")).unwrap();
        document
            .set_style(line, "overflow-wrap", Some("anywhere"))
            .unwrap();
        document
            .set_style(emphasized, "color", Some("#7c3aed"))
            .unwrap();
        document
            .set_style(emphasized, "font-weight", Some("700"))
            .unwrap();
        document.insert(BODY_ID, line, None).unwrap();
        document.insert(line, leading, None).unwrap();
        document.insert(line, emphasized, None).unwrap();
        document.insert(emphasized, emphasized_text, None).unwrap();
        document.insert(line, reactive, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let inline = &layout.children()[0];
        let LayoutKind::Text {
            text,
            spans,
            line_count,
            ..
        } = &inline.kind
        else {
            panic!("a text-only span tree should become one inline text layout");
        };

        assert_eq!(text, "Hello world 1");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].text, "world");
        assert_eq!(
            spans[1].style.color,
            Color::from_rgba8(0x7c, 0x3a, 0xed, 0xff)
        );
        assert_eq!(spans[1].style.font_weight, 700);
        assert!(*line_count > 1);
        assert!(inline.children().is_empty());

        document.set_text(reactive, " 2".into()).unwrap();
        let updated = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let LayoutKind::Text { text, .. } = &updated.children()[0].kind else {
            panic!("updated inline content should remain one text layout");
        };
        assert_eq!(text, "Hello world 2");
    }

    #[test]
    fn jsx_text_fragments_and_variables_share_a_nowrap_line() {
        let mut document = Document::new();
        let line = document.create_node(ElementKind::Span);
        let prefix = document.create_node(ElementKind::Text("Scroll item ".into()));
        let variable = document.create_node(ElementKind::Text("1".into()));
        let suffix = document.create_node(ElementKind::Text(
            " · drag either thumb or use the mouse wheel".into(),
        ));
        document.set_style(line, "width", Some("180px")).unwrap();
        document
            .set_style(line, "white-space", Some("nowrap"))
            .unwrap();
        document.insert(BODY_ID, line, None).unwrap();
        document.insert(line, prefix, None).unwrap();
        document.insert(line, variable, None).unwrap();
        document.insert(line, suffix, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let LayoutKind::Text {
            text,
            spans,
            style,
            line_count,
            ..
        } = &layout.children()[0].kind
        else {
            panic!("adjacent JSX text fragments should become one inline text layout");
        };

        assert_eq!(
            text,
            "Scroll item 1 · drag either thumb or use the mouse wheel"
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(style.wrap, TextWrap::None);
        assert_eq!(*line_count, 1);

        document.set_text(variable, "2".into()).unwrap();
        let updated = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let LayoutKind::Text {
            text, line_count, ..
        } = &updated.children()[0].kind
        else {
            panic!("the updated variable should remain in the inline text flow");
        };
        assert!(text.starts_with("Scroll item 2"));
        assert_eq!(*line_count, 1);
    }

    #[test]
    fn normalizes_text_according_to_white_space_and_wrapping_styles() {
        let mut document = Document::new();
        let normal_box = document.create_node(ElementKind::Div);
        let pre_box = document.create_node(ElementKind::Div);
        let anywhere_box = document.create_node(ElementKind::Div);
        let normal = document.create_node(ElementKind::Text("  a \n  b  ".into()));
        let pre = document.create_node(ElementKind::Text("  a \n  b  ".into()));
        let anywhere = document.create_node(ElementKind::Text("abcdefghij".into()));
        document
            .set_style(pre_box, "white-space", Some("pre"))
            .unwrap();
        document
            .set_style(anywhere_box, "overflow-wrap", Some("anywhere"))
            .unwrap();
        document.insert(BODY_ID, normal_box, None).unwrap();
        document.insert(BODY_ID, pre_box, None).unwrap();
        document.insert(BODY_ID, anywhere_box, None).unwrap();
        document.insert(normal_box, normal, None).unwrap();
        document.insert(pre_box, pre, None).unwrap();
        document.insert(anywhere_box, anywhere, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let children = layout.children();
        let LayoutKind::Text { text, style, .. } = &children[0].children()[0].kind else {
            panic!("normal should be text");
        };
        assert_eq!(text, "a b");
        assert_eq!(style.wrap, TextWrap::Word);
        let LayoutKind::Text { text, style, .. } = &children[1].children()[0].kind else {
            panic!("pre should be text");
        };
        assert_eq!(text, "  a \n  b  ");
        assert_eq!(style.wrap, TextWrap::None);
        let LayoutKind::Text { style, .. } = &children[2].children()[0].kind else {
            panic!("anywhere should be text");
        };
        assert_eq!(style.wrap, TextWrap::Glyph);
    }

    #[test]
    fn explicit_normal_wrapping_values_override_inherited_aggressive_values() {
        let mut document = Document::new();
        let anywhere = document.create_node(ElementKind::Div);
        let normal_overflow = document.create_node(ElementKind::Div);
        let anywhere_text = document.create_node(ElementKind::Text("abcdefgh".into()));
        document
            .set_style(anywhere, "overflow-wrap", Some("anywhere"))
            .unwrap();
        document
            .set_style(normal_overflow, "overflow-wrap", Some("normal"))
            .unwrap();

        let break_all = document.create_node(ElementKind::Div);
        let normal_break = document.create_node(ElementKind::Div);
        let normal_break_text = document.create_node(ElementKind::Text("abcdefgh".into()));
        document
            .set_style(break_all, "word-break", Some("break-all"))
            .unwrap();
        document
            .set_style(normal_break, "word-break", Some("normal"))
            .unwrap();

        let inherited_break_all = document.create_node(ElementKind::Div);
        let keep_all = document.create_node(ElementKind::Div);
        let keep_all_text = document.create_node(ElementKind::Text("abcdefgh".into()));
        document
            .set_style(inherited_break_all, "word-break", Some("break-all"))
            .unwrap();
        document
            .set_style(keep_all, "word-break", Some("keep-all"))
            .unwrap();

        for (outer, inner, text) in [
            (anywhere, normal_overflow, anywhere_text),
            (break_all, normal_break, normal_break_text),
            (inherited_break_all, keep_all, keep_all_text),
        ] {
            document.insert(BODY_ID, outer, None).unwrap();
            document.insert(outer, inner, None).unwrap();
            document.insert(inner, text, None).unwrap();
        }

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        for outer in layout.children() {
            let text = &outer.children()[0].children()[0];
            let LayoutKind::Text { style, .. } = &text.kind else {
                panic!("nested child should be text");
            };
            assert_eq!(style.wrap, TextWrap::Word);
        }
    }

    #[test]
    fn propagates_glyphon_baselines_for_flex_alignment() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let small_box = document.create_node(ElementKind::Div);
        let large_box = document.create_node(ElementKind::Div);
        let small = document.create_node(ElementKind::Text("small".into()));
        let large = document.create_node(ElementKind::Text("large".into()));
        document.set_style(row, "display", Some("flex")).unwrap();
        document
            .set_style(row, "align-items", Some("baseline"))
            .unwrap();
        document
            .set_style(small_box, "font-size", Some("12px"))
            .unwrap();
        document
            .set_style(large_box, "font-size", Some("32px"))
            .unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, small_box, None).unwrap();
        document.insert(row, large_box, None).unwrap();
        document.insert(small_box, small, None).unwrap();
        document.insert(large_box, large, None).unwrap();

        let mut text_system = TextSystem::new();
        let layout = compute_layout(&document, 300.0, 100.0, &mut text_system);
        let children = layout.children()[0].children();
        let small_layout = &children[0].children()[0];
        let large_layout = &children[1].children()[0];
        let LayoutKind::Text {
            text: small_text,
            style: small_style,
            ..
        } = &small_layout.kind
        else {
            panic!("small should be text");
        };
        let LayoutKind::Text {
            text: large_text,
            style: large_style,
            ..
        } = &large_layout.kind
        else {
            panic!("large should be text");
        };
        let small_baseline = text_system
            .measure(small_text, small_style, TextConstraints::UNCONSTRAINED)
            .first_baseline;
        let large_baseline = text_system
            .measure(large_text, large_style, TextConstraints::UNCONSTRAINED)
            .first_baseline;

        assert!((small_layout.y + small_baseline - large_layout.y - large_baseline).abs() < 0.01);
    }

    #[test]
    fn block_baseline_comes_from_the_first_eligible_descendant() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let multi = document.create_node(ElementKind::Div);
        let first_box = document.create_node(ElementKind::Div);
        let second_box = document.create_node(ElementKind::Div);
        let reference_box = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Text("first".into()));
        let second = document.create_node(ElementKind::Text("second".into()));
        let reference = document.create_node(ElementKind::Text("reference".into()));
        document.set_style(row, "display", Some("flex")).unwrap();
        document
            .set_style(row, "align-items", Some("baseline"))
            .unwrap();
        document
            .set_style(first_box, "font-size", Some("12px"))
            .unwrap();
        document
            .set_style(second_box, "font-size", Some("24px"))
            .unwrap();
        document
            .set_style(reference_box, "font-size", Some("32px"))
            .unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, multi, None).unwrap();
        document.insert(row, reference_box, None).unwrap();
        document.insert(multi, first_box, None).unwrap();
        document.insert(multi, second_box, None).unwrap();
        document.insert(first_box, first, None).unwrap();
        document.insert(second_box, second, None).unwrap();
        document.insert(reference_box, reference, None).unwrap();

        let mut text_system = TextSystem::new();
        let layout = compute_layout(&document, 300.0, 150.0, &mut text_system);
        let row_children = layout.children()[0].children();
        let first_layout = &row_children[0].children()[0].children()[0];
        let reference_layout = &row_children[1].children()[0];
        let LayoutKind::Text {
            text: first_text,
            style: first_style,
            ..
        } = &first_layout.kind
        else {
            panic!("first descendant should be text");
        };
        let LayoutKind::Text {
            text: reference_text,
            style: reference_style,
            ..
        } = &reference_layout.kind
        else {
            panic!("reference should be text");
        };
        let first_baseline = text_system
            .measure(first_text, first_style, TextConstraints::UNCONSTRAINED)
            .first_baseline;
        let reference_baseline = text_system
            .measure(
                reference_text,
                reference_style,
                TextConstraints::UNCONSTRAINED,
            )
            .first_baseline;

        assert!(
            (first_layout.y + first_baseline - reference_layout.y - reference_baseline).abs()
                < 0.01
        );
    }

    #[test]
    fn returns_absolute_coordinates_for_nested_boxes() {
        let mut document = Document::new();
        let outer = document.create_node(ElementKind::Div);
        let inner = document.create_node(ElementKind::Div);
        document
            .set_style(outer, "margin-left", Some("30px"))
            .unwrap();
        document
            .set_style(outer, "padding-left", Some("12px"))
            .unwrap();
        document.set_style(inner, "width", Some("50px")).unwrap();
        document.set_style(inner, "height", Some("20px")).unwrap();
        document.insert(BODY_ID, outer, None).unwrap();
        document.insert(outer, inner, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let outer = &layout.kind.children()[0];
        let inner = &outer.kind.children()[0];

        assert_eq!(outer.x, 30.0);
        assert_eq!(inner.x, 42.0);
        assert_eq!((inner.width, inner.height), (50.0, 20.0));
    }

    #[test]
    fn carries_z_index_and_isolation_into_layout() {
        let mut document = Document::new();
        let indexed = document.create_node(ElementKind::Div);
        let isolated = document.create_node(ElementKind::Div);
        document.set_style(indexed, "z-index", Some("-7")).unwrap();
        document
            .set_style(isolated, "isolation", Some("isolate"))
            .unwrap();
        document.insert(BODY_ID, indexed, None).unwrap();
        document.insert(BODY_ID, isolated, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let children = layout.children();

        let LayoutKind::Box {
            z_index, isolated, ..
        } = &children[0].kind
        else {
            panic!("indexed child should be a box");
        };
        assert_eq!(*z_index, Some(-7));
        assert!(!isolated);

        let LayoutKind::Box {
            z_index, isolated, ..
        } = &children[1].kind
        else {
            panic!("isolated child should be a box");
        };
        assert_eq!(*z_index, None);
        assert!(*isolated);
    }

    #[test]
    fn parent_transform_moves_descendants_clips_and_hit_testing_around_parent_center() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Div);
        document.set_style(parent, "width", Some("100px")).unwrap();
        document.set_style(parent, "height", Some("100px")).unwrap();
        document
            .set_style(parent, "transform", Some("rotate(90deg)"))
            .unwrap();
        document
            .set_style(parent, "overflow", Some("hidden"))
            .unwrap();
        document.set_style(child, "width", Some("20px")).unwrap();
        document.set_style(child, "height", Some("10px")).unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        let layout = compute_layout(&document, 200.0, 200.0, &mut TextSystem::new());
        let parent = &layout.children()[0];
        let child = &parent.children()[0];

        assert!(child.contains(95.0, 10.0));
        assert!(!child.contains(10.0, 5.0));
        assert_eq!(child.clips.len(), 1);
        assert!(child.clips[0].contains(95.0, 10.0));
        assert!(!child.clips[0].contains(10.0, 120.0));
    }

    #[test]
    fn computes_explicit_grid_tracks_and_named_placement() {
        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let item = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document.set_style(grid, "width", Some("300px")).unwrap();
        document.set_style(grid, "height", Some("100px")).unwrap();
        document
            .set_style(
                grid,
                "grid-template-columns",
                Some("[left] 80px [content] 120px [right]"),
            )
            .unwrap();
        document
            .set_style(grid, "grid-template-rows", Some("40px 60px"))
            .unwrap();
        document
            .set_style(item, "grid-column", Some("content / right"))
            .unwrap();
        document.set_style(item, "grid-row", Some("2")).unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, item, None).unwrap();

        let layout = compute_layout(&document, 400.0, 200.0, &mut TextSystem::new());
        let grid = &layout.children()[0];
        let item = &grid.children()[0];

        assert_eq!((grid.width, grid.height), (300.0, 100.0));
        assert_eq!((item.x, item.y), (80.0, 40.0));
        assert_eq!((item.width, item.height), (120.0, 60.0));
    }

    #[test]
    fn computes_implicit_grid_tracks_with_column_auto_flow() {
        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        let third = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document.set_style(grid, "width", Some("200px")).unwrap();
        document.set_style(grid, "height", Some("60px")).unwrap();
        document
            .set_style(grid, "grid-template-rows", Some("30px 30px"))
            .unwrap();
        document
            .set_style(grid, "grid-auto-columns", Some("40px"))
            .unwrap();
        document
            .set_style(grid, "grid-auto-flow", Some("column"))
            .unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, first, None).unwrap();
        document.insert(grid, second, None).unwrap();
        document.insert(grid, third, None).unwrap();

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();

        assert_eq!(
            children
                .iter()
                .map(|child| (child.x, child.y, child.width, child.height))
                .collect::<Vec<_>>(),
            vec![
                (0.0, 0.0, 40.0, 30.0),
                (0.0, 30.0, 40.0, 30.0),
                (40.0, 0.0, 40.0, 30.0),
            ]
        );
    }

    #[test]
    fn computes_named_grid_template_areas() {
        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let header = document.create_node(ElementKind::Div);
        let main = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document.set_style(grid, "width", Some("300px")).unwrap();
        document.set_style(grid, "height", Some("100px")).unwrap();
        document
            .set_style(grid, "grid-template-columns", Some("100px 200px"))
            .unwrap();
        document
            .set_style(grid, "grid-template-rows", Some("40px 60px"))
            .unwrap();
        document
            .set_style(
                grid,
                "grid-template-areas",
                Some("\"header header\" \"sidebar main\""),
            )
            .unwrap();
        document
            .set_style(header, "grid-area", Some("header"))
            .unwrap();
        document.set_style(main, "grid-area", Some("main")).unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, header, None).unwrap();
        document.insert(grid, main, None).unwrap();

        let layout = compute_layout(&document, 400.0, 200.0, &mut TextSystem::new());
        let children = layout.children()[0].children();

        assert_eq!(
            (
                children[0].x,
                children[0].y,
                children[0].width,
                children[0].height
            ),
            (0.0, 0.0, 300.0, 40.0)
        );
        assert_eq!(
            (
                children[1].x,
                children[1].y,
                children[1].width,
                children[1].height
            ),
            (100.0, 40.0, 200.0, 60.0)
        );
    }

    #[test]
    fn flex_shorthand_controls_growth() {
        let mut document = Document::new();
        let flex = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(flex, "display", Some("flex")).unwrap();
        document.set_style(flex, "width", Some("300px")).unwrap();
        document.set_style(first, "flex", Some("2")).unwrap();
        document.set_style(second, "flex", Some("1")).unwrap();
        document.insert(BODY_ID, flex, None).unwrap();
        document.insert(flex, first, None).unwrap();
        document.insert(flex, second, None).unwrap();

        let layout = compute_layout(&document, 400.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();

        assert_eq!(children[0].width, 200.0);
        assert_eq!(children[1].width, 100.0);
        assert_eq!(children[1].x, 200.0);
    }

    #[test]
    fn order_reorders_flex_items_stably_but_not_block_children() {
        let mut document = Document::new();
        let flex = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        let third = document.create_node(ElementKind::Div);
        document.set_style(flex, "display", Some("flex")).unwrap();
        document.set_style(first, "width", Some("40px")).unwrap();
        document.set_style(second, "width", Some("40px")).unwrap();
        document.set_style(third, "width", Some("40px")).unwrap();
        document.set_style(first, "order", Some("2")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.set_style(third, "order", Some("2")).unwrap();
        document.insert(BODY_ID, flex, None).unwrap();
        document.insert(flex, first, None).unwrap();
        document.insert(flex, second, None).unwrap();
        document.insert(flex, third, None).unwrap();

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();
        assert_eq!(
            children
                .iter()
                .map(|child| child.element_id)
                .collect::<Vec<_>>(),
            vec![second, first, third]
        );
        assert_eq!(
            children.iter().map(|child| child.x).collect::<Vec<_>>(),
            vec![0.0, 40.0, 80.0]
        );

        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document
            .set_style(grid, "grid-template-columns", Some("40px 40px"))
            .unwrap();
        document.set_style(first, "order", Some("1")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, first, None).unwrap();
        document.insert(grid, second, None).unwrap();
        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();
        assert_eq!(
            children
                .iter()
                .map(|child| (child.element_id, child.x))
                .collect::<Vec<_>>(),
            vec![(second, 0.0), (first, 40.0)]
        );

        let mut document = Document::new();
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(first, "order", Some("2")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.insert(BODY_ID, first, None).unwrap();
        document.insert(BODY_ID, second, None).unwrap();
        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        assert_eq!(
            layout
                .children()
                .iter()
                .map(|child| child.element_id)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
    }
}
