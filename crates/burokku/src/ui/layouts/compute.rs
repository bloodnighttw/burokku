use std::collections::HashMap;

use render::{
    Border, BoxStyle, Clip, Color, CornerRadius, FontFamily, Outline, Rect as RenderRect,
    TextConstraints, TextStyle, TextSystem,
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
        AlignItems, BoxSizing, Color as ElementColor, Display as ElementDisplay, FlexDirection,
        JustifyContent, LengthPercentageValue, LengthValue, LineHeightValue, MaxSizeValue,
        Overflow as ElementOverflow, SizeValue, Style as ElementStyle,
    },
    Document, Element, ElementKind, BODY_ID,
};

use super::{
    Layout, LayoutKind, NativeAppearance, ScrollContainer, ScrollOffset, Scrollbar, ScrollbarAxis,
    StackingLayer,
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
    let style = native_style(document, element_id, element);
    let text_style = merge_text_style(inherited_text_style, &style);
    let native_appearance = native_appearance(element, &style, &text_style);
    let node_id = nodes.len();
    nodes.push(LayoutNode {
        element_id,
        kind: element.kind.clone(),
        style: to_taffy_style(&element.kind, &style),
        paint_style: style,
        text_style: text_style.clone(),
        native_appearance,
        children: Vec::with_capacity(element.children.len()),
        cache: Cache::new(),
        layout: TaffyLayout::new(),
    });

    let children = element
        .children
        .iter()
        .map(|child| add_element(nodes, document, *child, &text_style))
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
    native_appearance: Option<NativeAppearance>,
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
            | ElementKind::Option
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
                        native_appearance: data.native_appearance,
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

fn native_style(document: &Document, element_id: u64, element: &Element) -> ElementStyle {
    let mut style = element.style.clone();
    match element.kind {
        ElementKind::Button => {
            set_default(
                &mut style.display,
                ElementDisplay::Flex,
                element,
                &["display"],
            );
            set_default(
                &mut style.box_sizing,
                BoxSizing::BorderBox,
                element,
                &["box-sizing"],
            );
            set_default(
                &mut style.align_items,
                Some(AlignItems::CENTER),
                element,
                &["align-items"],
            );
            set_default(
                &mut style.justify_content,
                Some(JustifyContent::CENTER),
                element,
                &["justify-content"],
            );
            native_control_box(&mut style, element, 8.0, 24.0);
            apply_control_state_colors(&mut style, element, false);
        }
        ElementKind::Select => {
            set_default(
                &mut style.display,
                ElementDisplay::Flex,
                element,
                &["display"],
            );
            set_default(
                &mut style.box_sizing,
                BoxSizing::BorderBox,
                element,
                &["box-sizing"],
            );
            set_default(
                &mut style.align_items,
                Some(AlignItems::CENTER),
                element,
                &["align-items"],
            );
            let multiple = element.attributes.contains_key("multiple");
            native_control_box(&mut style, element, if multiple { 8.0 } else { 24.0 }, 28.0);
            if multiple {
                set_default(
                    &mut style.flex_direction,
                    FlexDirection::Column,
                    element,
                    &["flex-direction"],
                );
                set_default(
                    &mut style.align_items,
                    Some(AlignItems::STRETCH),
                    element,
                    &["align-items"],
                );
            }
            set_default(
                &mut style.background_color,
                Some([255, 255, 255, 255]),
                element,
                &["background-color"],
            );
            apply_control_state_colors(&mut style, element, true);
        }
        ElementKind::Option => {
            let multiple = option_select(document, element_id)
                .is_some_and(|select| select.attributes.contains_key("multiple"));
            if !multiple && !option_is_selected(document, element_id) {
                style.display = ElementDisplay::None;
            } else {
                if multiple {
                    set_default(
                        &mut style.padding_top,
                        LengthPercentageValue::Px(2.0),
                        element,
                        &["padding", "padding-top"],
                    );
                    set_default(
                        &mut style.padding_right,
                        LengthPercentageValue::Px(6.0),
                        element,
                        &["padding", "padding-right"],
                    );
                    set_default(
                        &mut style.padding_bottom,
                        LengthPercentageValue::Px(2.0),
                        element,
                        &["padding", "padding-bottom"],
                    );
                    set_default(
                        &mut style.padding_left,
                        LengthPercentageValue::Px(6.0),
                        element,
                        &["padding", "padding-left"],
                    );
                }
                apply_option_state_colors(
                    &mut style,
                    element,
                    option_is_selected(document, element_id),
                );
            }
        }
        _ => {}
    }
    style
}

fn native_appearance(
    element: &Element,
    style: &ElementStyle,
    text_style: &TextStyle,
) -> Option<NativeAppearance> {
    let disabled = element.attributes.contains_key("disabled");
    let focused = element.attributes.contains_key("data-burokku-focused");
    let active = !disabled
        && (element.attributes.contains_key("data-burokku-active")
            || element
                .attributes
                .get("aria-pressed")
                .is_some_and(|value| value == "true"));
    let border_color = style
        .border_color
        .map_or(Color::from_rgba8(118, 118, 118, 255), rgba);
    let default_borders = native_default_borders(element);
    let border_widths = [
        style.border_top_width.px(),
        style.border_right_width.px(),
        style.border_bottom_width.px(),
        style.border_left_width.px(),
    ];
    match element.kind {
        ElementKind::Button => Some(NativeAppearance::Button {
            disabled,
            focused,
            active,
            default_borders,
            border_widths,
            border_color,
        }),
        ElementKind::Select => Some(NativeAppearance::Select {
            color: text_style.color,
            disabled,
            focused,
            multiple: element.attributes.contains_key("multiple"),
            default_borders,
            border_widths,
            border_color,
        }),
        _ => None,
    }
}

fn native_default_borders(element: &Element) -> [bool; 4] {
    if !matches!(element.kind, ElementKind::Button | ElementKind::Select) {
        return [false; 4];
    }
    ["top", "right", "bottom", "left"].map(|side| {
        !element.specified_styles.contains("border-style")
            && !element
                .specified_styles
                .contains(&format!("border-{side}-style"))
    })
}

fn apply_control_state_colors(style: &mut ElementStyle, element: &Element, select: bool) {
    let disabled = element.attributes.contains_key("disabled");
    let focused = element.attributes.contains_key("data-burokku-focused");
    let active = !disabled
        && (element.attributes.contains_key("data-burokku-active")
            || element
                .attributes
                .get("aria-pressed")
                .is_some_and(|value| value == "true"));

    if disabled {
        set_default(
            &mut style.background_color,
            Some(if select {
                [245, 245, 245, 255]
            } else {
                [232, 232, 232, 255]
            }),
            element,
            &["background-color"],
        );
        set_default(
            &mut style.color,
            Some([112, 112, 112, 255]),
            element,
            &["color"],
        );
        set_default(
            &mut style.border_color,
            Some([180, 180, 180, 255]),
            element,
            &["border-color"],
        );
    } else if active && !select {
        set_default(
            &mut style.background_color,
            Some([214, 214, 214, 255]),
            element,
            &["background-color"],
        );
    }

    if focused {
        set_default(
            &mut style.outline_color,
            Some([38, 132, 255, 255]),
            element,
            &["outline-color"],
        );
        set_default(
            &mut style.outline_width,
            LengthValue::Px(2.0),
            element,
            &["outline-width"],
        );
        set_default(
            &mut style.outline_offset,
            LengthValue::Px(1.0),
            element,
            &["outline-offset"],
        );
    }
}

fn apply_option_state_colors(style: &mut ElementStyle, element: &Element, selected: bool) {
    if element.attributes.contains_key("disabled") {
        set_default(
            &mut style.color,
            Some([128, 128, 128, 255]),
            element,
            &["color"],
        );
    } else if selected {
        set_default(
            &mut style.background_color,
            Some([38, 132, 255, 255]),
            element,
            &["background-color"],
        );
        set_default(
            &mut style.color,
            Some([255, 255, 255, 255]),
            element,
            &["color"],
        );
    }
}

fn native_control_box(
    style: &mut ElementStyle,
    element: &Element,
    right_padding: f32,
    min_height: f32,
) {
    set_default(
        &mut style.min_height,
        SizeValue::Px(min_height),
        element,
        &["min-height"],
    );
    set_default(
        &mut style.padding_top,
        LengthPercentageValue::Px(3.0),
        element,
        &["padding", "padding-top"],
    );
    set_default(
        &mut style.padding_right,
        LengthPercentageValue::Px(right_padding),
        element,
        &["padding", "padding-right"],
    );
    set_default(
        &mut style.padding_bottom,
        LengthPercentageValue::Px(3.0),
        element,
        &["padding", "padding-bottom"],
    );
    set_default(
        &mut style.padding_left,
        LengthPercentageValue::Px(8.0),
        element,
        &["padding", "padding-left"],
    );
    for (width, property) in [
        (&mut style.border_top_width, "border-top-width"),
        (&mut style.border_right_width, "border-right-width"),
        (&mut style.border_bottom_width, "border-bottom-width"),
        (&mut style.border_left_width, "border-left-width"),
    ] {
        set_default(
            width,
            LengthValue::Px(1.0),
            element,
            &["border-width", property],
        );
    }
    set_default(
        &mut style.background_color,
        Some([239, 239, 239, 255]),
        element,
        &["background-color"],
    );
    set_default(&mut style.color, Some([0, 0, 0, 255]), element, &["color"]);
    set_default(
        &mut style.border_color,
        Some([118, 118, 118, 255]),
        element,
        &["border-color"],
    );
    for (radius, property) in [
        (&mut style.border_top_left_radius, "border-top-left-radius"),
        (
            &mut style.border_top_right_radius,
            "border-top-right-radius",
        ),
        (
            &mut style.border_bottom_right_radius,
            "border-bottom-right-radius",
        ),
        (
            &mut style.border_bottom_left_radius,
            "border-bottom-left-radius",
        ),
    ] {
        set_default(
            radius,
            LengthPercentageValue::Px(3.0),
            element,
            &["border-radius", property],
        );
    }
}

fn set_default<T>(field: &mut T, value: T, element: &Element, properties: &[&str]) {
    if !properties
        .iter()
        .any(|property| element.specified_styles.contains(*property))
    {
        *field = value;
    }
}

fn option_is_selected(document: &Document, option_id: u64) -> bool {
    let option = document
        .node(option_id)
        .expect("layout nodes always refer to live document nodes");
    let Some(select) = option_select(document, option_id) else {
        return true;
    };

    let options = select.children.iter().filter(|child| {
        document
            .node(**child)
            .is_ok_and(|child| matches!(child.kind, ElementKind::Option))
    });
    if select
        .attributes
        .contains_key("data-burokku-selection-explicit")
    {
        let is_dynamic_selection = |child: &&u64| {
            document
                .node(**child)
                .is_ok_and(|child| child.attributes.contains_key("data-burokku-selected"))
        };
        if select.attributes.contains_key("multiple") {
            return options
                .clone()
                .any(|child| child == &option_id && is_dynamic_selection(&child));
        }
        return options
            .clone()
            .rfind(is_dynamic_selection)
            .is_some_and(|child| *child == option_id);
    }

    if select.attributes.contains_key("multiple") {
        let has_selected = options.clone().any(|child| {
            document
                .node(*child)
                .is_ok_and(option_is_initially_selected)
        });
        if has_selected {
            return option_is_initially_selected(option);
        }
        return false;
    }

    let selected = options.clone().rfind(|child| {
        document
            .node(**child)
            .is_ok_and(option_is_initially_selected)
    });
    selected
        .or_else(|| {
            options.clone().find(|child| {
                document
                    .node(**child)
                    .is_ok_and(|child| !child.attributes.contains_key("disabled"))
            })
        })
        .or_else(|| options.clone().next())
        .copied()
        == Some(option_id)
}

fn option_is_initially_selected(option: &Element) -> bool {
    option.attributes.contains_key("data-burokku-selected")
        || (!option
            .attributes
            .contains_key("data-burokku-option-explicit")
            && option.attributes.contains_key("selected"))
}

fn option_select(document: &Document, option_id: u64) -> Option<&Element> {
    let option = document.node(option_id).ok()?;
    let select = document.node(option.parent?).ok()?;
    matches!(select.kind, ElementKind::Select).then_some(select)
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
    fn buttons_receive_native_defaults_without_overriding_author_styles() {
        let mut document = Document::new();
        let button = document.create_node(ElementKind::Button);
        document
            .set_style(button, "background-color", Some("#123456"))
            .unwrap();
        document
            .set_style(button, "padding-left", Some("2px"))
            .unwrap();

        let element = document.node(button).unwrap();
        let style = native_style(&document, button, element);
        assert_eq!(style.display, ElementDisplay::Flex);
        assert_eq!(style.min_height, SizeValue::Px(24.0));
        assert_eq!(style.padding_left, LengthPercentageValue::Px(2.0));
        assert_eq!(style.padding_right, LengthPercentageValue::Px(8.0));
        assert_eq!(style.background_color, Some([0x12, 0x34, 0x56, 0xff]));
        assert_eq!(style.border_top_width, LengthValue::Px(1.0));
    }

    #[test]
    fn selects_project_only_the_selected_option_into_closed_layout() {
        let mut document = Document::new();
        let select = document.create_node(ElementKind::Select);
        let first = document.create_node(ElementKind::Option);
        let second = document.create_node(ElementKind::Option);
        let first_text = document.create_node(ElementKind::Text("First".into()));
        let second_text = document.create_node(ElementKind::Text("Second".into()));
        document.insert(BODY_ID, select, None).unwrap();
        document.insert(select, first, None).unwrap();
        document.insert(select, second, None).unwrap();
        document.insert(first, first_text, None).unwrap();
        document.insert(second, second_text, None).unwrap();
        document
            .set_attribute(second, "selected", Some(""))
            .unwrap();

        assert_eq!(
            native_style(&document, first, document.node(first).unwrap()).display,
            ElementDisplay::None
        );
        assert_ne!(
            native_style(&document, second, document.node(second).unwrap()).display,
            ElementDisplay::None
        );

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let select_layout = &layout.children()[0];
        assert!(select_layout.height >= 28.0);
        assert_eq!(
            select_layout
                .iter()
                .filter(|layout| matches!(&layout.kind, LayoutKind::Text { text, .. } if text == "Second"))
                .count(),
            1
        );
        assert!(select_layout.iter().all(
            |layout| !matches!(&layout.kind, LayoutKind::Text { text, .. } if text == "First")
                || layout.width == 0.0
        ));
    }

    #[test]
    fn explicit_empty_selection_does_not_restore_the_first_option() {
        let mut document = Document::new();
        let select = document.create_node(ElementKind::Select);
        let first = document.create_node(ElementKind::Option);
        let second = document.create_node(ElementKind::Option);
        document.insert(BODY_ID, select, None).unwrap();
        document.insert(select, first, None).unwrap();
        document.insert(select, second, None).unwrap();
        document
            .set_attribute(select, "data-burokku-selection-explicit", Some(""))
            .unwrap();

        assert!(!option_is_selected(&document, first));
        assert!(!option_is_selected(&document, second));
        assert_eq!(
            native_style(&document, first, document.node(first).unwrap()).display,
            ElementDisplay::None
        );
        assert_eq!(
            native_style(&document, second, document.node(second).unwrap()).display,
            ElementDisplay::None
        );
    }

    #[test]
    fn multiple_selects_show_all_options_and_style_selected_and_disabled_rows() {
        let mut document = Document::new();
        let select = document.create_node(ElementKind::Select);
        let selected = document.create_node(ElementKind::Option);
        let disabled = document.create_node(ElementKind::Option);
        document.insert(BODY_ID, select, None).unwrap();
        document.insert(select, selected, None).unwrap();
        document.insert(select, disabled, None).unwrap();
        document
            .set_attribute(select, "multiple", Some(""))
            .unwrap();
        document
            .set_attribute(select, "data-burokku-selection-explicit", Some(""))
            .unwrap();
        document
            .set_attribute(selected, "data-burokku-selected", Some(""))
            .unwrap();
        document
            .set_attribute(disabled, "disabled", Some(""))
            .unwrap();

        let selected_style = native_style(&document, selected, document.node(selected).unwrap());
        let disabled_style = native_style(&document, disabled, document.node(disabled).unwrap());
        assert_ne!(selected_style.display, ElementDisplay::None);
        assert_ne!(disabled_style.display, ElementDisplay::None);
        assert_eq!(selected_style.background_color, Some([38, 132, 255, 255]));
        assert_eq!(selected_style.color, Some([255, 255, 255, 255]));
        assert_eq!(disabled_style.color, Some([128, 128, 128, 255]));
    }

    #[test]
    fn native_border_defaults_are_per_side_and_yield_to_author_properties() {
        let mut element = Element {
            kind: ElementKind::Button,
            parent: None,
            children: Vec::new(),
            style: ElementStyle::default(),
            attributes: HashMap::new(),
            specified_styles: Default::default(),
        };
        element.specified_styles.insert("border-top-style".into());
        element.specified_styles.insert("border-left-width".into());

        assert_eq!(native_default_borders(&element), [false, true, true, true]);
    }

    #[test]
    fn disabled_focused_and_pressed_controls_receive_distinct_native_states() {
        let mut document = Document::new();
        let disabled = document.create_node(ElementKind::Button);
        let active = document.create_node(ElementKind::Button);
        let focused = document.create_node(ElementKind::Select);
        document
            .set_attribute(disabled, "disabled", Some(""))
            .unwrap();
        document
            .set_attribute(active, "aria-pressed", Some("true"))
            .unwrap();
        document
            .set_attribute(focused, "data-burokku-focused", Some(""))
            .unwrap();

        let disabled_style = native_style(&document, disabled, document.node(disabled).unwrap());
        let active_style = native_style(&document, active, document.node(active).unwrap());
        let focused_style = native_style(&document, focused, document.node(focused).unwrap());
        assert_eq!(disabled_style.color, Some([112, 112, 112, 255]));
        assert_eq!(disabled_style.background_color, Some([232, 232, 232, 255]));
        assert_eq!(active_style.background_color, Some([214, 214, 214, 255]));
        assert_eq!(focused_style.outline_color, Some([38, 132, 255, 255]));
        assert_eq!(focused_style.outline_width, LengthValue::Px(2.0));
    }
}
