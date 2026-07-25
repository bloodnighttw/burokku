use std::collections::HashMap;

use render::{
    Border, BoxStyle, Clip, Color, CornerRadius, FontFamily, FontStyle, Outline,
    Rect as RenderRect, TextAlign, TextConstraints, TextDecorationLine, TextStyle, TextSystem,
    TextWhiteSpace, TextWrap,
};
use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout,
    geometry::{Point, Rect, Size},
    prelude::{
        AvailableSpace, Dimension, Display, LengthPercentage, LengthPercentageAuto, NodeId,
        TaffyAuto,
    },
    style::{
        GridAutoTracks, GridPlacement, GridTemplateComponent, GridTemplateTracks,
        Style as TaffyStyle,
    },
    tree::{
        Cache, Layout as TaffyLayout, LayoutBlockContainer, LayoutFlexboxContainer,
        LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, RunMode,
        TraversePartialTree,
    },
    BlockContext, CacheTree,
};

use crate::ui::elements::{
    styles::{
        Color as ElementColor, FontStyleValue, LengthPercentageValue, LineHeightValue,
        MaxSizeValue, Overflow as ElementOverflow, OverflowWrapValue, SizeValue,
        Style as ElementStyle, TextAlignValue, TextDecorationLineValue, WhiteSpaceValue,
        WordBreakValue,
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
    let node_id = nodes.len();
    nodes.push(LayoutNode {
        element_id,
        kind: element.kind.clone(),
        style: to_taffy_style(&element.kind, &element.style),
        paint_style: element.style.clone(),
        text_style: text_style.clone(),
        children: Vec::with_capacity(element.children.len()),
        cache: Cache::new(),
        layout: TaffyLayout::new(),
        first_baseline: None,
        text_line_count: 0,
    });

    let children = element
        .children
        .iter()
        .map(|child| add_element(nodes, document, *child, &text_style))
        .collect::<Vec<_>>();
    let mut children = children;
    if matches!(element.style.display, Display::Flex | Display::Grid) {
        children.sort_by_key(|child| nodes[*child].paint_style.order);
    }
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
    first_baseline: Option<f32>,
    text_line_count: usize,
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
            let is_text = matches!(tree.nodes[index].kind, ElementKind::Text(_));
            let has_children = !tree.nodes[index].children.is_empty();

            match (display, is_text, has_children) {
                (Display::None, _, _) => compute_hidden_layout(tree, node_id),
                (_, true, _) => tree.compute_text_layout(node_id, inputs),
                (Display::Block, false, true) => {
                    let mut output = compute_block_layout(tree, node_id, inputs, block_context);
                    output.first_baselines.y =
                        tree.nodes[index].children.iter().rev().find_map(|child| {
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
        let text = match &self.nodes[index].kind {
            ElementKind::Text(text) => text.clone(),
            _ => unreachable!("only text elements use text measurement"),
        };

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
                let text = normalize_white_space(&text, text_style.white_space);
                let measured = self.text_system.measure(&text, &text_style, constraints);
                first_baseline = Some(measured.first_baseline);
                self.nodes[index].text_line_count = measured.line_count;
                Size {
                    width: known_dimensions.width.unwrap_or(measured.width),
                    height: known_dimensions.height.unwrap_or(measured.height),
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
    ) -> Layout {
        let data = &self.nodes[node];
        let location = Point {
            x: parent_location.x + data.layout.location.x,
            y: parent_location.y + data.layout.location.y,
        };
        let width = data.layout.size.width;
        let height = data.layout.size.height;
        let mut descendant_clips = ancestor_clips.to_vec();
        let own_clip = overflow_clip(data, location, width, height, viewport);
        if let Some(clip) = own_clip {
            descendant_clips.push(clip);
        }
        let (kind, scroll) = match &data.kind {
            ElementKind::Text(text) => (
                LayoutKind::Text {
                    text: normalize_white_space(text, data.text_style.white_space),
                    style: data.text_style.clone(),
                    line_count: data.text_line_count,
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
                let mut children: Vec<_> = data
                    .children
                    .iter()
                    .map(|child| self.to_layout(*child, child_parent, &descendant_clips, viewport))
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
                            self.to_layout(*child, child_parent, &descendant_clips, viewport)
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
            clips: ancestor_clips.to_vec(),
            scroll,
            kind,
        }
    }
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
    children.iter().fold(
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
        CornerRadius::new(
            (outer.top_left - border.left.max(border.top)).max(0.0),
            (outer.top_right - border.right.max(border.top)).max(0.0),
            (outer.bottom_right - border.right.max(border.bottom)).max(0.0),
            (outer.bottom_left - border.left.max(border.bottom)).max(0.0),
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

fn to_taffy_style(kind: &ElementKind, style: &ElementStyle) -> TaffyStyle {
    let (grid_template_rows, grid_template_row_names) =
        grid_template(style.grid_template_rows.as_deref());
    let (grid_template_columns, grid_template_column_names) =
        grid_template(style.grid_template_columns.as_deref());
    TaffyStyle {
        display: if matches!(kind, ElementKind::Comment(_)) {
            Display::None
        } else {
            style.display
        },
        box_sizing: style.box_sizing,
        position: style.position,
        overflow: Point {
            x: taffy_overflow(style.overflow_x),
            y: taffy_overflow(style.overflow_y),
        },
        inset: Rect {
            left: length_percentage_auto(style.left),
            right: length_percentage_auto(style.right),
            top: length_percentage_auto(style.top),
            bottom: length_percentage_auto(style.bottom),
        },
        size: Size {
            width: dimension(style.width),
            height: dimension(style.height),
        },
        min_size: Size {
            width: dimension(style.min_width),
            height: dimension(style.min_height),
        },
        max_size: Size {
            width: max_dimension(style.max_width),
            height: max_dimension(style.max_height),
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
            left: LengthPercentage::length(style.border_left_width.px()),
            right: LengthPercentage::length(style.border_right_width.px()),
            top: LengthPercentage::length(style.border_top_width.px()),
            bottom: LengthPercentage::length(style.border_bottom_width.px()),
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
        grid_template_rows,
        grid_template_columns,
        grid_template_areas: style.grid_template_areas.clone(),
        grid_template_row_names,
        grid_template_column_names,
        grid_auto_rows: grid_auto_tracks(style.grid_auto_rows.as_deref()),
        grid_auto_columns: grid_auto_tracks(style.grid_auto_columns.as_deref()),
        grid_auto_flow: style.grid_auto_flow,
        grid_row: taffy::geometry::Line {
            start: grid_placement(style.grid_row_start.as_deref()),
            end: grid_placement(style.grid_row_end.as_deref()),
        },
        grid_column: taffy::geometry::Line {
            start: grid_placement(style.grid_column_start.as_deref()),
            end: grid_placement(style.grid_column_end.as_deref()),
        },
        ..TaffyStyle::default()
    }
}

type TemplateTracks = GridTemplateTracks<String, GridTemplateComponent<String>>;

fn grid_template(value: Option<&str>) -> (Vec<GridTemplateComponent<String>>, Vec<Vec<String>>) {
    value.map_or_else(
        || (Vec::new(), Vec::new()),
        |value| {
            let parsed = value
                .parse::<TemplateTracks>()
                .expect("grid templates are validated when styles are set");
            (parsed.tracks, parsed.line_names)
        },
    )
}

fn grid_auto_tracks(value: Option<&str>) -> Vec<taffy::style::TrackSizingFunction> {
    value.map_or_else(Vec::new, |value| {
        value
            .parse::<GridAutoTracks>()
            .expect("implicit grid tracks are validated when styles are set")
            .0
    })
}

fn grid_placement(value: Option<&str>) -> GridPlacement<String> {
    value.map_or(GridPlacement::Auto, |value| {
        value
            .parse()
            .expect("grid placement is validated when styles are set")
    })
}

fn dimension(value: SizeValue) -> Dimension {
    match value {
        SizeValue::Auto => Dimension::AUTO,
        SizeValue::Px(value) => Dimension::length(value),
        SizeValue::Percent(value) => Dimension::percent(value / 100.0),
    }
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
        if merged.text_decoration_color_is_current {
            merged.text_decoration_color = merged.color;
        }
    }
    if let Some(font_size) = style.font_size {
        merged.font_size = match font_size {
            LengthPercentageValue::Px(value) => value,
            LengthPercentageValue::Percent(value) => parent.font_size * value / 100.0,
        };
        if merged.line_height_is_normal && style.line_height.is_none() {
            merged.line_height = merged.font_size * 1.2;
        }
    }
    if let Some(line_height) = style.line_height {
        merged.line_height_is_normal = line_height == LineHeightValue::Normal;
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
    if let Some(font_families) = &style.font_families {
        merged.font_families = font_families
            .iter()
            .map(|family| font_family(family))
            .collect();
    }
    if let Some(font_style) = style.font_style {
        merged.font_style = match font_style {
            FontStyleValue::Normal => FontStyle::Normal,
            FontStyleValue::Italic => FontStyle::Italic,
            FontStyleValue::Oblique => FontStyle::Oblique,
        };
    }
    if let Some(text_align) = style.text_align {
        merged.text_align = match text_align {
            TextAlignValue::Start => TextAlign::Start,
            TextAlignValue::End => TextAlign::End,
            TextAlignValue::Left => TextAlign::Left,
            TextAlignValue::Right => TextAlign::Right,
            TextAlignValue::Center => TextAlign::Center,
            TextAlignValue::Justify => TextAlign::Justify,
        };
    }
    if let Some(letter_spacing) = style.letter_spacing {
        merged.letter_spacing = letter_spacing.px();
    }
    if let Some(word_spacing) = style.word_spacing {
        merged.word_spacing = word_spacing.px();
    }
    if let Some(line) = style.text_decoration_line {
        merged.text_decoration_line = text_decoration_line(line);
    }
    if let Some(color) = style.text_decoration_color {
        merged.text_decoration_color = rgba(color);
        merged.text_decoration_color_is_current = false;
    }
    if let Some(white_space) = style.white_space {
        (merged.white_space, merged.wrap) = match white_space {
            WhiteSpaceValue::Normal => (TextWhiteSpace::Collapse, TextWrap::Word),
            WhiteSpaceValue::NoWrap => (TextWhiteSpace::Collapse, TextWrap::None),
            WhiteSpaceValue::Pre => (TextWhiteSpace::Preserve, TextWrap::None),
            WhiteSpaceValue::PreWrap => (TextWhiteSpace::Preserve, TextWrap::Word),
            WhiteSpaceValue::PreLine => (TextWhiteSpace::CollapsePreserveNewlines, TextWrap::Word),
            WhiteSpaceValue::BreakSpaces => (TextWhiteSpace::Preserve, TextWrap::Glyph),
        };
    }
    if let Some(overflow_wrap) = style.overflow_wrap {
        match overflow_wrap {
            OverflowWrapValue::Normal => {}
            OverflowWrapValue::BreakWord => merged.wrap = TextWrap::WordOrGlyph,
            OverflowWrapValue::Anywhere => merged.wrap = TextWrap::Glyph,
        }
    }
    if let Some(word_break) = style.word_break {
        match word_break {
            WordBreakValue::Normal | WordBreakValue::KeepAll => {}
            WordBreakValue::BreakAll => merged.wrap = TextWrap::Glyph,
        }
    }
    merged
}

fn font_family(family: &str) -> FontFamily {
    match family.to_ascii_lowercase().as_str() {
        "serif" => FontFamily::Serif,
        "sans-serif" => FontFamily::SansSerif,
        "monospace" => FontFamily::Monospace,
        "cursive" => FontFamily::Cursive,
        "fantasy" => FontFamily::Fantasy,
        _ => FontFamily::Named(family.to_owned()),
    }
}

fn text_decoration_line(value: TextDecorationLineValue) -> TextDecorationLine {
    let mut result = TextDecorationLine::NONE;
    for (source, target) in [
        (
            TextDecorationLineValue::UNDERLINE,
            TextDecorationLine::UNDERLINE,
        ),
        (
            TextDecorationLineValue::OVERLINE,
            TextDecorationLine::OVERLINE,
        ),
        (
            TextDecorationLineValue::LINE_THROUGH,
            TextDecorationLine::LINE_THROUGH,
        ),
    ] {
        if value.contains(source) {
            result = result.union(target);
        }
    }
    result
}

fn normalize_white_space(text: &str, mode: TextWhiteSpace) -> String {
    match mode {
        TextWhiteSpace::Preserve => text.to_owned(),
        TextWhiteSpace::Collapse => collapse_white_space(text, false),
        TextWhiteSpace::CollapsePreserveNewlines => collapse_white_space(text, true),
    }
}

fn collapse_white_space(text: &str, preserve_newlines: bool) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character == '\n' && preserve_newlines {
            while result.ends_with(' ') {
                result.pop();
            }
            result.push('\n');
            pending_space = false;
        } else if character.is_whitespace() {
            pending_space = !result.is_empty() && !result.ends_with('\n');
        } else {
            if pending_space {
                result.push(' ');
            }
            result.push(character);
            pending_space = false;
        }
    }
    result
}

fn box_style(style: &ElementStyle, width: f32, height: f32) -> BoxStyle {
    let border_width = [
        style.border_top_width.px(),
        style.border_right_width.px(),
        style.border_bottom_width.px(),
        style.border_left_width.px(),
    ]
    .into_iter()
    .reduce(f32::max)
    .unwrap_or(0.0);

    BoxStyle {
        background: style.background_color.map_or(Color::TRANSPARENT, rgba),
        corner_radius: CornerRadius::new(
            radius(style.border_top_left_radius, width, height),
            radius(style.border_top_right_radius, width, height),
            radius(style.border_bottom_right_radius, width, height),
            radius(style.border_bottom_left_radius, width, height),
        ),
        border: (border_width > 0.0)
            .then(|| Border::new(border_width, style.border_color.map_or(Color::BLACK, rgba))),
        outline: (style.outline_width.px() > 0.0).then(|| {
            Outline::new(
                style.outline_width.px(),
                style.outline_offset.px(),
                style.outline_color.map_or(Color::BLACK, rgba),
            )
        }),
    }
}

fn radius(value: LengthPercentageValue, width: f32, height: f32) -> f32 {
    match value {
        LengthPercentageValue::Px(value) => value,
        LengthPercentageValue::Percent(value) => width.min(height) * value / 100.0,
    }
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
