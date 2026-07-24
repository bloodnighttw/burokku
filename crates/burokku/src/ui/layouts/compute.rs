use std::collections::HashMap;

use render::{
    Border, BorderSide, BorderStyle as RenderBorderStyle, BoxStyle, Clip, Color, CornerRadius,
    CornerSize, FontFamily, Outline, Rect as RenderRect, TextConstraints, TextStyle, TextSystem,
};
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout,
    geometry::{Point, Rect, Size},
    prelude::{
        AvailableSpace, Dimension, Display, LengthPercentage, LengthPercentageAuto, NodeId,
        TaffyAuto,
    },
    style::Style as TaffyStyle,
    tree::{
        Cache, Layout as TaffyLayout, LayoutBlockContainer, LayoutFlexboxContainer,
        LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, RunMode,
        TraversePartialTree,
    },
    BlockContext, CacheTree,
};

use crate::ui::elements::{
    styles::{
        BorderStyle as ElementBorderStyle, Color as ElementColor, CornerRadiusValue,
        LengthPercentageValue, LineHeightValue, MaxSizeValue, Overflow as ElementOverflow,
        Position as ElementPosition, SizeValue, Style as ElementStyle,
    },
    Document, ElementKind, BODY_ID,
};

use super::{
    Layout, LayoutKind, ScrollContainer, ScrollOffset, Scrollbar, ScrollbarAxis, StackingLayer,
};

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
    let root = add_element(
        &mut nodes,
        document,
        BODY_ID,
        &TextStyle::default(),
        viewport,
    );
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
        RenderRect::new(0.0, 0.0, viewport.width, viewport.height),
    )
}

fn add_element(
    nodes: &mut Vec<LayoutNode>,
    document: &Document,
    element_id: u64,
    inherited_text_style: &TextStyle,
    viewport: Size<f32>,
) -> usize {
    let element = document
        .node(element_id)
        .expect("element child IDs are validated when inserted");
    let text_style = merge_text_style(inherited_text_style, &element.style);
    let node_id = nodes.len();
    nodes.push(LayoutNode {
        element_id,
        kind: element.kind.clone(),
        style: to_taffy_style(&element.kind, &element.style, viewport),
        paint_style: element.style.clone(),
        text_style: text_style.clone(),
        children: Vec::with_capacity(element.children.len()),
        cache: Cache::new(),
        layout: TaffyLayout::new(),
    });

    let children = element
        .children
        .iter()
        .map(|child| add_element(nodes, document, *child, &text_style, viewport))
        .collect();
    nodes[node_id].children = children;
    node_id
}

struct LayoutNode {
    element_id: u64,
    kind: ElementKind,
    style: TaffyStyle,
    paint_style: ElementStyle,
    text_style: TextStyle,
    children: Vec<usize>,
    cache: Cache,
    layout: TaffyLayout,
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

        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let index = usize::from(node_id);
            let display = tree.nodes[index].style.display;
            let is_text = matches!(tree.nodes[index].kind, ElementKind::Text(_));
            let has_children = !tree.nodes[index].children.is_empty();

            match (display, is_text, has_children) {
                (Display::None, _, _) => compute_hidden_layout(tree, node_id),
                (_, true, _) => tree.compute_text_layout(node_id, inputs),
                (Display::Block, false, true) => {
                    compute_block_layout(tree, node_id, inputs, block_context)
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
        })
    }

    fn compute_text_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let index = usize::from(node_id);
        let style = self.nodes[index].style.clone();
        let text_style = self.nodes[index].text_style.clone();
        let text = match &self.nodes[index].kind {
            ElementKind::Text(text) => text.clone(),
            _ => unreachable!("only text elements use text measurement"),
        };

        compute_leaf_layout(
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
                let measured = self.text_system.measure(&text, &text_style, constraints);
                Size {
                    width: known_dimensions.width.unwrap_or(measured.width),
                    height: known_dimensions.height.unwrap_or(measured.height),
                }
            },
        )
    }

    fn to_layout(
        &self,
        node: usize,
        parent_location: Point<f32>,
        ancestor_clips: &[Clip],
        viewport: RenderRect,
        positioned_ancestor: RenderRect,
    ) -> Layout {
        let data = &self.nodes[node];
        let taffy_location = Point {
            x: parent_location.x + data.layout.location.x,
            y: parent_location.y + data.layout.location.y,
        };
        let width = data.layout.size.width;
        let height = data.layout.size.height;
        let containing_block = if data.paint_style.position == ElementPosition::Fixed {
            viewport
        } else {
            positioned_ancestor
        };
        let location = positioned_location(
            &data.paint_style,
            taffy_location,
            width,
            height,
            containing_block,
        );
        let own_ancestor_clips = if data.paint_style.position == ElementPosition::Fixed {
            &[][..]
        } else {
            ancestor_clips
        };
        let mut descendant_clips = own_ancestor_clips.to_vec();
        let own_clip = overflow_clip(data, location, width, height, viewport);
        if let Some(clip) = own_clip {
            descendant_clips.push(clip);
        }
        let (kind, scroll) = match &data.kind {
            ElementKind::Text(text) => (
                LayoutKind::Text {
                    text: text.clone(),
                    style: data.text_style.clone(),
                },
                None,
            ),
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
                let own_positioned_box = padding_box(data, location, width, height);
                let child_positioned_ancestor =
                    if data.paint_style.position == ElementPosition::Static {
                        translated_rect(positioned_ancestor, -offset.x, -offset.y)
                    } else {
                        translated_rect(own_positioned_box, -offset.x, -offset.y)
                    };
                let mut children: Vec<_> = data
                    .children
                    .iter()
                    .map(|child| {
                        self.to_layout(
                            *child,
                            child_parent,
                            &descendant_clips,
                            viewport,
                            child_positioned_ancestor,
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
                    let child_positioned_ancestor =
                        if data.paint_style.position == ElementPosition::Static {
                            translated_rect(positioned_ancestor, -offset.x, -offset.y)
                        } else {
                            translated_rect(own_positioned_box, -offset.x, -offset.y)
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
                                child_positioned_ancestor,
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
                        offset,
                        max_offset,
                        data.paint_style.overflow_x == ElementOverflow::Scroll,
                        data.paint_style.overflow_y == ElementOverflow::Scroll,
                    )
                });
                (
                    LayoutKind::Box {
                        style: box_style(&data.paint_style, width, height),
                        stacking_layer: StackingLayer::from_style(&data.paint_style),
                        children,
                    },
                    scroll,
                )
            }
        };

        Layout {
            element_id: data.element_id,
            x: location.x,
            y: location.y,
            width,
            height,
            clips: own_ancestor_clips.to_vec(),
            scroll,
            is_fixed: data.paint_style.position == ElementPosition::Fixed,
            kind,
        }
    }
}

fn positioned_location(
    style: &ElementStyle,
    fallback: Point<f32>,
    width: f32,
    height: f32,
    containing_block: RenderRect,
) -> Point<f32> {
    if matches!(
        style.position,
        ElementPosition::Static | ElementPosition::Relative
    ) {
        return fallback;
    }
    Point {
        x: positioned_axis(
            style.left,
            style.right,
            style.margin_left,
            style.margin_right,
            containing_block.x,
            containing_block.width,
            width,
        )
        .unwrap_or(fallback.x),
        y: positioned_axis(
            style.top,
            style.bottom,
            style.margin_top,
            style.margin_bottom,
            containing_block.y,
            containing_block.height,
            height,
        )
        .unwrap_or(fallback.y),
    }
}

fn positioned_axis(
    start: SizeValue,
    end: SizeValue,
    start_margin: SizeValue,
    end_margin: SizeValue,
    origin: f32,
    containing_size: f32,
    own_size: f32,
) -> Option<f32> {
    resolve_position(start, containing_size)
        .map(|offset| origin + offset + resolve_margin(start_margin, containing_size))
        .or_else(|| {
            resolve_position(end, containing_size).map(|offset| {
                origin + containing_size
                    - offset
                    - own_size
                    - resolve_margin(end_margin, containing_size)
            })
        })
}

fn resolve_position(value: SizeValue, basis: f32) -> Option<f32> {
    match value {
        SizeValue::Auto => None,
        SizeValue::Px(value) => Some(value),
        SizeValue::Percent(value) => Some(basis * value / 100.0),
    }
}

fn resolve_margin(value: SizeValue, basis: f32) -> f32 {
    resolve_position(value, basis).unwrap_or(0.0)
}

fn translated_rect(rect: RenderRect, x: f32, y: f32) -> RenderRect {
    RenderRect::new(rect.x + x, rect.y + y, rect.width, rect.height)
}

fn padding_box(data: &LayoutNode, location: Point<f32>, width: f32, height: f32) -> RenderRect {
    let border = data.layout.border;
    RenderRect::new(
        location.x + border.left,
        location.y + border.top,
        (width - border.left - border.right).max(0.0),
        (height - border.top - border.bottom).max(0.0),
    )
}

fn scroll_content_size(
    children: &[Layout],
    viewport: RenderRect,
    offset: ScrollOffset,
) -> (f32, f32) {
    children.iter().filter(|child| !child.is_fixed).fold(
        (viewport.width, viewport.height),
        |(width, height), child| {
            (
                width.max(child.x + offset.x + child.width - viewport.x),
                height.max(child.y + offset.y + child.height - viewport.y),
            )
        },
    )
}

fn scroll_container(
    viewport: RenderRect,
    clip: Clip,
    content_width: f32,
    content_height: f32,
    offset: ScrollOffset,
    max_offset: ScrollOffset,
    always_show_horizontal: bool,
    always_show_vertical: bool,
) -> ScrollContainer {
    const INSET: f32 = 2.0;
    const THICKNESS: f32 = 8.0;
    const CROSS_AXIS_SPACE: f32 = 12.0;
    const MIN_THUMB: f32 = 24.0;

    let has_horizontal = always_show_horizontal || max_offset.x > 0.0;
    let has_vertical = always_show_vertical || max_offset.y > 0.0;
    let horizontal = has_horizontal.then(|| {
        let track = RenderRect::new(
            viewport.x + INSET,
            viewport.y + viewport.height - THICKNESS - INSET,
            (viewport.width - INSET * 2.0 - if has_vertical { CROSS_AXIS_SPACE } else { 0.0 })
                .max(0.0),
            THICKNESS,
        );
        Scrollbar {
            axis: ScrollbarAxis::Horizontal,
            track,
            thumb: scrollbar_thumb(
                track,
                ScrollbarAxis::Horizontal,
                viewport.width,
                content_width,
                offset.x,
                max_offset.x,
                MIN_THUMB,
            ),
        }
    });
    let vertical = has_vertical.then(|| {
        let track = RenderRect::new(
            viewport.x + viewport.width - THICKNESS - INSET,
            viewport.y + INSET,
            THICKNESS,
            (viewport.height
                - INSET * 2.0
                - if has_horizontal {
                    CROSS_AXIS_SPACE
                } else {
                    0.0
                })
            .max(0.0),
        );
        Scrollbar {
            axis: ScrollbarAxis::Vertical,
            track,
            thumb: scrollbar_thumb(
                track,
                ScrollbarAxis::Vertical,
                viewport.height,
                content_height,
                offset.y,
                max_offset.y,
                MIN_THUMB,
            ),
        }
    });

    ScrollContainer {
        viewport,
        clip,
        content_width,
        content_height,
        offset,
        max_offset,
        horizontal,
        vertical,
    }
}

fn scrollbar_thumb(
    track: RenderRect,
    axis: ScrollbarAxis,
    viewport_size: f32,
    content_size: f32,
    offset: f32,
    max_offset: f32,
    min_thumb: f32,
) -> RenderRect {
    let track_size = match axis {
        ScrollbarAxis::Horizontal => track.width,
        ScrollbarAxis::Vertical => track.height,
    };
    let thumb_size = (track_size * viewport_size / content_size.max(viewport_size))
        .clamp(min_thumb.min(track_size), track_size);
    let travel = (track_size - thumb_size).max(0.0);
    let position = if max_offset > 0.0 {
        travel * offset / max_offset
    } else {
        0.0
    };
    match axis {
        ScrollbarAxis::Horizontal => {
            RenderRect::new(track.x + position, track.y, thumb_size, track.height)
        }
        ScrollbarAxis::Vertical => {
            RenderRect::new(track.x, track.y + position, track.width, thumb_size)
        }
    }
}

fn overflow_clip(
    data: &LayoutNode,
    location: Point<f32>,
    width: f32,
    height: f32,
    viewport: RenderRect,
) -> Option<Clip> {
    let clips_x = data.paint_style.overflow_x != ElementOverflow::Visible;
    let clips_y = data.paint_style.overflow_y != ElementOverflow::Visible;
    if !clips_x && !clips_y {
        return None;
    }

    let border = data.layout.border;
    let padding_box = padding_box(data, location, width, height);
    let rect = RenderRect::new(
        if clips_x { padding_box.x } else { viewport.x },
        if clips_y { padding_box.y } else { viewport.y },
        if clips_x {
            padding_box.width
        } else {
            viewport.width
        },
        if clips_y {
            padding_box.height
        } else {
            viewport.height
        },
    );
    let corner_radius = if clips_x && clips_y {
        let outer = box_style(&data.paint_style, width, height).corner_radius;
        CornerRadius::elliptical(
            CornerSize::new(
                (outer.top_left.x - border.left).max(0.0),
                (outer.top_left.y - border.top).max(0.0),
            ),
            CornerSize::new(
                (outer.top_right.x - border.right).max(0.0),
                (outer.top_right.y - border.top).max(0.0),
            ),
            CornerSize::new(
                (outer.bottom_right.x - border.right).max(0.0),
                (outer.bottom_right.y - border.bottom).max(0.0),
            ),
            CornerSize::new(
                (outer.bottom_left.x - border.left).max(0.0),
                (outer.bottom_left.y - border.bottom).max(0.0),
            ),
        )
    } else {
        CornerRadius::ZERO
    };
    Some(Clip::new(rect, corner_radius))
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

fn to_taffy_style(kind: &ElementKind, style: &ElementStyle, viewport: Size<f32>) -> TaffyStyle {
    TaffyStyle {
        display: if matches!(kind, ElementKind::Comment(_)) {
            Display::None
        } else {
            style.display
        },
        box_sizing: style.box_sizing,
        position: match style.position {
            ElementPosition::Static | ElementPosition::Relative => taffy::style::Position::Relative,
            ElementPosition::Absolute | ElementPosition::Fixed => taffy::style::Position::Absolute,
        },
        overflow: Point {
            x: taffy_overflow(style.overflow_x),
            y: taffy_overflow(style.overflow_y),
        },
        inset: if style.position == ElementPosition::Static {
            Rect {
                left: LengthPercentageAuto::AUTO,
                right: LengthPercentageAuto::AUTO,
                top: LengthPercentageAuto::AUTO,
                bottom: LengthPercentageAuto::AUTO,
            }
        } else {
            Rect {
                left: length_percentage_auto(style.left),
                right: length_percentage_auto(style.right),
                top: length_percentage_auto(style.top),
                bottom: length_percentage_auto(style.bottom),
            }
        },
        size: Size {
            width: fixed_size_dimension(
                style.width,
                style.left,
                style.right,
                style.margin_left,
                style.margin_right,
                viewport.width,
                style.position,
            ),
            height: fixed_size_dimension(
                style.height,
                style.top,
                style.bottom,
                style.margin_top,
                style.margin_bottom,
                viewport.height,
                style.position,
            ),
        },
        min_size: Size {
            width: positioned_dimension(style.min_width, viewport.width, style.position),
            height: positioned_dimension(style.min_height, viewport.height, style.position),
        },
        max_size: Size {
            width: positioned_max_dimension(style.max_width, viewport.width, style.position),
            height: positioned_max_dimension(style.max_height, viewport.height, style.position),
        },
        aspect_ratio: style.aspect_ratio,
        margin: Rect {
            left: length_percentage_auto(style.margin_left),
            right: length_percentage_auto(style.margin_right),
            top: length_percentage_auto(style.margin_top),
            bottom: length_percentage_auto(style.margin_bottom),
        },
        padding: Rect {
            left: length_percentage(style.padding_left),
            right: length_percentage(style.padding_right),
            top: length_percentage(style.padding_top),
            bottom: length_percentage(style.padding_bottom),
        },
        border: Rect {
            left: LengthPercentage::length(effective_border_width(
                style.border_left_width.px(),
                style.border_left_style,
            )),
            right: LengthPercentage::length(effective_border_width(
                style.border_right_width.px(),
                style.border_right_style,
            )),
            top: LengthPercentage::length(effective_border_width(
                style.border_top_width.px(),
                style.border_top_style,
            )),
            bottom: LengthPercentage::length(effective_border_width(
                style.border_bottom_width.px(),
                style.border_bottom_style,
            )),
        },
        align_content: style.align_content,
        align_items: style.align_items,
        align_self: style.align_self,
        justify_content: style.justify_content,
        gap: Size {
            width: length_percentage(style.column_gap),
            height: length_percentage(style.row_gap),
        },
        flex_direction: style.flex_direction,
        flex_wrap: style.flex_wrap,
        flex_basis: dimension(style.flex_basis),
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        ..TaffyStyle::default()
    }
}

fn dimension(value: SizeValue) -> Dimension {
    match value {
        SizeValue::Auto => Dimension::AUTO,
        SizeValue::Px(value) => Dimension::length(value),
        SizeValue::Percent(value) => Dimension::percent(value / 100.0),
    }
}

fn positioned_dimension(value: SizeValue, viewport: f32, position: ElementPosition) -> Dimension {
    if position == ElementPosition::Fixed {
        match value {
            SizeValue::Percent(value) => Dimension::length(viewport * value / 100.0),
            _ => dimension(value),
        }
    } else {
        dimension(value)
    }
}

fn fixed_size_dimension(
    value: SizeValue,
    start: SizeValue,
    end: SizeValue,
    start_margin: SizeValue,
    end_margin: SizeValue,
    viewport: f32,
    position: ElementPosition,
) -> Dimension {
    if position == ElementPosition::Fixed && value == SizeValue::Auto {
        if let (Some(start), Some(end)) = (
            resolve_position(start, viewport),
            resolve_position(end, viewport),
        ) {
            return Dimension::length(
                (viewport
                    - start
                    - end
                    - resolve_margin(start_margin, viewport)
                    - resolve_margin(end_margin, viewport))
                .max(0.0),
            );
        }
    }
    positioned_dimension(value, viewport, position)
}

fn taffy_overflow(value: ElementOverflow) -> taffy::style::Overflow {
    match value {
        ElementOverflow::Visible => taffy::style::Overflow::Visible,
        ElementOverflow::Hidden => taffy::style::Overflow::Hidden,
        ElementOverflow::Clip => taffy::style::Overflow::Clip,
        ElementOverflow::Auto | ElementOverflow::Scroll => taffy::style::Overflow::Scroll,
    }
}

fn max_dimension(value: MaxSizeValue) -> Dimension {
    match value {
        MaxSizeValue::None => Dimension::AUTO,
        MaxSizeValue::Px(value) => Dimension::length(value),
        MaxSizeValue::Percent(value) => Dimension::percent(value / 100.0),
    }
}

fn positioned_max_dimension(
    value: MaxSizeValue,
    viewport: f32,
    position: ElementPosition,
) -> Dimension {
    if position == ElementPosition::Fixed {
        match value {
            MaxSizeValue::Percent(value) => Dimension::length(viewport * value / 100.0),
            _ => max_dimension(value),
        }
    } else {
        max_dimension(value)
    }
}

fn length_percentage(value: LengthPercentageValue) -> LengthPercentage {
    match value {
        LengthPercentageValue::Px(value) => LengthPercentage::length(value),
        LengthPercentageValue::Percent(value) => LengthPercentage::percent(value / 100.0),
    }
}

fn length_percentage_auto(value: SizeValue) -> LengthPercentageAuto {
    match value {
        SizeValue::Auto => LengthPercentageAuto::AUTO,
        SizeValue::Px(value) => LengthPercentageAuto::length(value),
        SizeValue::Percent(value) => LengthPercentageAuto::percent(value / 100.0),
    }
}

fn merge_text_style(parent: &TextStyle, style: &ElementStyle) -> TextStyle {
    let mut merged = parent.clone();
    if let Some(color) = style.color {
        merged.color = rgba(color);
    }
    if let Some(font_size) = style.font_size {
        merged.font_size = match font_size {
            LengthPercentageValue::Px(value) => value,
            LengthPercentageValue::Percent(value) => parent.font_size * value / 100.0,
        };
    }
    if let Some(line_height) = style.line_height {
        merged.line_height = match line_height {
            LineHeightValue::Normal => merged.font_size * 1.2,
            LineHeightValue::Number(value) => merged.font_size * value,
            LineHeightValue::Px(value) => value,
            LineHeightValue::Percent(value) => merged.font_size * value / 100.0,
        };
    }
    if let Some(font_weight) = style.font_weight {
        merged.font_weight = font_weight;
    }
    if let Some(font_family) = &style.font_family {
        merged.font_family = FontFamily::Named(font_family.clone());
    }
    merged
}

fn box_style(style: &ElementStyle, width: f32, height: f32) -> BoxStyle {
    let border_widths = [
        effective_border_width(style.border_top_width.px(), style.border_top_style),
        effective_border_width(style.border_right_width.px(), style.border_right_style),
        effective_border_width(style.border_bottom_width.px(), style.border_bottom_style),
        effective_border_width(style.border_left_width.px(), style.border_left_style),
    ];

    BoxStyle {
        background: style.background_color.map_or(Color::TRANSPARENT, rgba),
        corner_radius: CornerRadius::elliptical(
            radius(style.border_top_left_radius, width, height),
            radius(style.border_top_right_radius, width, height),
            radius(style.border_bottom_right_radius, width, height),
            radius(style.border_bottom_left_radius, width, height),
        ),
        border: border_widths.iter().any(|width| *width > 0.0).then(|| {
            Border::sides(
                border_side(
                    border_widths[0],
                    style.border_top_color,
                    style.border_top_style,
                ),
                border_side(
                    border_widths[1],
                    style.border_right_color,
                    style.border_right_style,
                ),
                border_side(
                    border_widths[2],
                    style.border_bottom_color,
                    style.border_bottom_style,
                ),
                border_side(
                    border_widths[3],
                    style.border_left_color,
                    style.border_left_style,
                ),
            )
        }),
        outline: (style.outline_width.px() > 0.0).then(|| {
            Outline::new(
                style.outline_width.px(),
                style.outline_offset.px(),
                style.outline_color.map_or(Color::BLACK, rgba),
            )
        }),
    }
}

fn radius(value: CornerRadiusValue, width: f32, height: f32) -> CornerSize {
    CornerSize::new(
        resolve_radius(value.horizontal, width),
        resolve_radius(value.vertical, height),
    )
}

fn resolve_radius(value: LengthPercentageValue, basis: f32) -> f32 {
    match value {
        LengthPercentageValue::Px(value) => value,
        LengthPercentageValue::Percent(value) => basis * value / 100.0,
    }
}

fn effective_border_width(width: f32, style: ElementBorderStyle) -> f32 {
    if matches!(style, ElementBorderStyle::None | ElementBorderStyle::Hidden) {
        0.0
    } else {
        width
    }
}

fn border_side(width: f32, color: Option<ElementColor>, style: ElementBorderStyle) -> BorderSide {
    BorderSide::new(
        width,
        color.map_or(Color::BLACK, rgba),
        match style {
            ElementBorderStyle::None => RenderBorderStyle::None,
            ElementBorderStyle::Hidden => RenderBorderStyle::Hidden,
            ElementBorderStyle::Dotted => RenderBorderStyle::Dotted,
            ElementBorderStyle::Dashed => RenderBorderStyle::Dashed,
            ElementBorderStyle::Solid => RenderBorderStyle::Solid,
            ElementBorderStyle::Double => RenderBorderStyle::Double,
            ElementBorderStyle::Groove => RenderBorderStyle::Groove,
            ElementBorderStyle::Ridge => RenderBorderStyle::Ridge,
            ElementBorderStyle::Inset => RenderBorderStyle::Inset,
            ElementBorderStyle::Outset => RenderBorderStyle::Outset,
        },
    )
}

fn rgba(color: ElementColor) -> Color {
    Color::from_rgba8(color[0], color[1], color[2], color[3])
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn carries_z_index_and_isolation_into_layout_layers() {
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

        assert_eq!(
            children[0].stacking_layer(),
            StackingLayer::new(Some(-7), false)
        );
        assert_eq!(children[1].stacking_layer(), StackingLayer::new(None, true));
    }

    #[test]
    fn static_ignores_insets_while_relative_offsets_its_flow_position() {
        let mut document = Document::new();
        let static_box = document.create_node(ElementKind::Div);
        let relative_box = document.create_node(ElementKind::Div);
        for element in [static_box, relative_box] {
            document.set_style(element, "width", Some("20px")).unwrap();
            document.set_style(element, "height", Some("20px")).unwrap();
            document.set_style(element, "left", Some("15px")).unwrap();
            document.set_style(element, "top", Some("10px")).unwrap();
            document.insert(BODY_ID, element, None).unwrap();
        }
        document
            .set_style(relative_box, "position", Some("relative"))
            .unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let children = layout.children();

        assert_eq!((children[0].x, children[0].y), (0.0, 0.0));
        assert_eq!((children[1].x, children[1].y), (15.0, 30.0));
    }

    #[test]
    fn absolute_insets_use_the_nearest_positioned_ancestor() {
        let mut document = Document::new();
        let positioned = document.create_node(ElementKind::Div);
        let static_wrapper = document.create_node(ElementKind::Div);
        let absolute = document.create_node(ElementKind::Div);
        document
            .set_style(positioned, "position", Some("relative"))
            .unwrap();
        document
            .set_style(positioned, "margin-left", Some("30px"))
            .unwrap();
        document
            .set_style(positioned, "width", Some("100px"))
            .unwrap();
        document
            .set_style(positioned, "height", Some("100px"))
            .unwrap();
        document
            .set_style(static_wrapper, "margin-left", Some("20px"))
            .unwrap();
        document
            .set_style(absolute, "position", Some("absolute"))
            .unwrap();
        document.set_style(absolute, "left", Some("5px")).unwrap();
        document.set_style(absolute, "top", Some("7px")).unwrap();
        document.set_style(absolute, "width", Some("10px")).unwrap();
        document
            .set_style(absolute, "height", Some("10px"))
            .unwrap();
        document.insert(BODY_ID, positioned, None).unwrap();
        document.insert(positioned, static_wrapper, None).unwrap();
        document.insert(static_wrapper, absolute, None).unwrap();

        let layout = compute_layout(&document, 200.0, 150.0, &mut TextSystem::new());
        let absolute = &layout.children()[0].children()[0].children()[0];

        assert_eq!((absolute.x, absolute.y), (35.0, 7.0));
    }

    #[test]
    fn fixed_insets_and_percent_sizes_use_the_viewport() {
        let mut document = Document::new();
        let wrapper = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        let stretched = document.create_node(ElementKind::Div);
        document
            .set_style(wrapper, "margin-left", Some("40px"))
            .unwrap();
        document
            .set_style(fixed, "position", Some("fixed"))
            .unwrap();
        document.set_style(fixed, "right", Some("10px")).unwrap();
        document.set_style(fixed, "bottom", Some("15px")).unwrap();
        document.set_style(fixed, "width", Some("50%")).unwrap();
        document.set_style(fixed, "height", Some("20px")).unwrap();
        document
            .set_style(stretched, "position", Some("fixed"))
            .unwrap();
        document.set_style(stretched, "left", Some("10px")).unwrap();
        document
            .set_style(stretched, "right", Some("20px"))
            .unwrap();
        document.set_style(stretched, "top", Some("0")).unwrap();
        document
            .set_style(stretched, "height", Some("10px"))
            .unwrap();
        document.insert(BODY_ID, wrapper, None).unwrap();
        document.insert(wrapper, fixed, None).unwrap();
        document.insert(wrapper, stretched, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let fixed = &layout.children()[0].children()[0];
        let stretched = &layout.children()[0].children()[1];

        assert_eq!(
            (fixed.x, fixed.y, fixed.width, fixed.height),
            (90.0, 65.0, 100.0, 20.0)
        );
        assert!(fixed.clips.is_empty());
        assert_eq!((stretched.x, stretched.width), (10.0, 170.0));
    }

    #[test]
    fn fixed_descendants_do_not_move_with_a_scroll_container() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        document
            .set_style(container, "position", Some("relative"))
            .unwrap();
        document
            .set_style(container, "overflow", Some("scroll"))
            .unwrap();
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("60px"))
            .unwrap();
        document.set_style(content, "width", Some("300px")).unwrap();
        document
            .set_style(content, "height", Some("200px"))
            .unwrap();
        document
            .set_style(fixed, "position", Some("fixed"))
            .unwrap();
        document.set_style(fixed, "left", Some("7px")).unwrap();
        document.set_style(fixed, "top", Some("5px")).unwrap();
        document.set_style(fixed, "width", Some("20px")).unwrap();
        document.set_style(fixed, "height", Some("10px")).unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, content, None).unwrap();
        document.insert(container, fixed, None).unwrap();
        let mut offsets = HashMap::new();
        offsets.insert(container, ScrollOffset::new(40.0, 30.0));

        let mut layout =
            compute_layout_with_scroll(&document, 200.0, 100.0, &mut TextSystem::new(), &offsets);
        let fixed_before = {
            let fixed = &layout.children()[0].children()[1];
            (fixed.x, fixed.y)
        };
        assert_eq!(fixed_before, (7.0, 5.0));

        assert!(layout.apply_scroll_offset(container, ScrollOffset::new(60.0, 50.0)));
        let fixed = &layout.children()[0].children()[1];
        assert_eq!((fixed.x, fixed.y), fixed_before);
    }
}
