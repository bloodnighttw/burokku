use std::collections::HashMap;

use render::{
    Border, BorderSide, BorderStyle as RenderBorderStyle, BoxShadow, BoxStyle, Canvas, Clip, Color,
    CornerRadius, CornerSize, Outline, Rect, TextDecorationLine, TextRunMetrics, TextShadow,
    TextStyle, TextSystem, Transform,
};

use super::{
    elements::Document,
    layouts::{
        compute_layout, compute_layout_with_scroll, Layout, LayoutKind, NativeAppearance,
        ScrollOffset,
    },
};

/// The computed UI geometry and the drawing commands produced from it.
#[derive(Clone, Debug, PartialEq)]
pub struct UiFrame {
    pub layout: Layout,
    pub canvas: Canvas,
}

/// Computes and preserves the document layout while building drawing commands.
pub fn build_frame(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> UiFrame {
    let layout = compute_layout(
        document,
        viewport_width.max(0.0),
        viewport_height.max(0.0),
        text_system,
    );
    frame_from_layout(layout, scale_factor)
}

pub(crate) fn build_frame_with_scroll(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
    scroll_offsets: &HashMap<u64, ScrollOffset>,
) -> UiFrame {
    let layout = compute_layout_with_scroll(
        document,
        viewport_width.max(0.0),
        viewport_height.max(0.0),
        text_system,
        scroll_offsets,
    );
    frame_from_layout(layout, scale_factor)
}

fn frame_from_layout(layout: Layout, scale_factor: f32) -> UiFrame {
    let canvas = canvas_from_layout(&layout, scale_factor);
    UiFrame { layout, canvas }
}

fn canvas_from_layout(layout: &Layout, scale_factor: f32) -> Canvas {
    let scale_factor = scale_factor.max(f32::EPSILON);
    let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
    let viewport = Rect::new(0.0, 0.0, layout.width, layout.height);
    paint_layout(layout, viewport, scale_factor, &mut canvas);
    paint_scrollbars(layout, viewport, scale_factor, &mut canvas);
    canvas
}

pub(crate) fn repaint_frame(frame: &mut UiFrame, scale_factor: f32) {
    frame.canvas = canvas_from_layout(&frame.layout, scale_factor);
}

/// Computes the document layout and converts it into renderer drawing commands.
pub fn build_canvas(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> Canvas {
    build_frame(
        document,
        viewport_width,
        viewport_height,
        scale_factor,
        text_system,
    )
    .canvas
}

fn paint_layout(layout: &Layout, viewport: Rect, scale_factor: f32, canvas: &mut Canvas) {
    for layout in layout {
        if layout.width <= 0.0 && layout.height <= 0.0 {
            continue;
        }

        let bounds = Rect::new(
            layout.x * scale_factor,
            layout.y * scale_factor,
            layout.width * scale_factor,
            layout.height * scale_factor,
        );
        let logical_bounds = visual_bounds(layout);
        if !intersects_visible_area(logical_bounds, &layout.clips, viewport) {
            continue;
        }
        let clips = layout
            .clips
            .iter()
            .copied()
            .map(|clip| scaled_clip(clip, scale_factor))
            .collect::<Vec<_>>();
        match &layout.kind {
            LayoutKind::Box {
                style,
                native_appearance,
                ..
            } => {
                if style != &BoxStyle::default() {
                    canvas.draw_box_with_clips(
                        bounds,
                        scaled_box_style(style.clone(), scale_factor),
                        clips.iter().copied(),
                    );
                }
                if let Some(appearance) = native_appearance {
                    paint_native_control(bounds, *appearance, scale_factor, &clips, canvas);
                }
            }
            LayoutKind::Text {
                text, style, runs, ..
            } => {
                let scaled_style = scaled_text_style(style, scale_factor);
                canvas.draw_text_with_clips(bounds, text, scaled_style.clone(), clips.clone());
                paint_text_decorations(bounds, &scaled_style, runs, scale_factor, clips, canvas);
            }
        }
    }
}

fn paint_text_decorations(
    bounds: Rect,
    style: &TextStyle,
    runs: &[TextRunMetrics],
    scale_factor: f32,
    clips: impl IntoIterator<Item = Clip> + Clone,
    canvas: &mut Canvas,
) {
    if style.text_decoration_line == TextDecorationLine::NONE {
        return;
    }
    let decoration_style = BoxStyle {
        background: style.text_decoration_color,
        ..BoxStyle::default()
    };
    for run in runs {
        for decoration in [
            style
                .text_decoration_line
                .contains(TextDecorationLine::OVERLINE)
                .then_some((run.overline_y, run.overline_thickness)),
            style
                .text_decoration_line
                .contains(TextDecorationLine::LINE_THROUGH)
                .then_some((run.line_through_y, run.line_through_thickness)),
            style
                .text_decoration_line
                .contains(TextDecorationLine::UNDERLINE)
                .then_some((run.underline_y, run.underline_thickness)),
        ]
        .into_iter()
        .flatten()
        {
            let (y, thickness) = decoration;
            canvas.draw_overlay_box_with_clips(
                Rect::new(
                    bounds.x + run.left * scale_factor,
                    bounds.y + y * scale_factor,
                    run.width * scale_factor,
                    thickness * scale_factor,
                ),
                decoration_style.clone(),
                clips.clone(),
            );
        }
    }
}

fn paint_native_control(
    bounds: Rect,
    appearance: NativeAppearance,
    scale_factor: f32,
    clips: &[Clip],
    canvas: &mut Canvas,
) {
    let (disabled, focused, active, default_borders, border_widths, border_colors) =
        match appearance {
            NativeAppearance::Button {
                disabled,
                focused,
                active,
                default_borders,
                border_widths,
                border_colors,
            } => (
                disabled,
                focused,
                active,
                default_borders,
                border_widths,
                border_colors,
            ),
            NativeAppearance::Select {
                disabled,
                focused,
                default_borders,
                border_widths,
                border_colors,
                ..
            } => (
                disabled,
                focused,
                false,
                default_borders,
                border_widths,
                border_colors,
            ),
        };

    paint_native_border(
        bounds,
        default_borders,
        border_widths,
        border_colors,
        scale_factor,
        clips,
        canvas,
    );

    if active || disabled || focused {
        let overlay = BoxStyle {
            background: if active {
                Color::from_rgba8(0, 0, 0, 20)
            } else if disabled {
                Color::from_rgba8(255, 255, 255, 35)
            } else {
                Color::TRANSPARENT
            },
            corner_radius: CornerRadius::all(3.0 * scale_factor),
            outline: focused.then(|| {
                Outline::new(
                    2.0 * scale_factor,
                    1.0 * scale_factor,
                    Color::from_rgba8(38, 132, 255, 255),
                )
            }),
            ..BoxStyle::default()
        };
        canvas.draw_overlay_box_with_clips(bounds, overlay, clips.iter().copied());
    }

    if let NativeAppearance::Select {
        color, multiple, ..
    } = appearance
    {
        if !multiple {
            paint_select_indicator(bounds, color, scale_factor, clips, canvas);
        }
    }
}

fn paint_native_border(
    bounds: Rect,
    sides: [bool; 4],
    widths: [f32; 4],
    colors: [Color; 4],
    scale_factor: f32,
    clips: &[Clip],
    canvas: &mut Canvas,
) {
    let widths: [f32; 4] = std::array::from_fn(|index| {
        if sides[index] {
            widths[index] * scale_factor
        } else {
            0.0
        }
    });
    if widths.iter().all(|width| *width <= 0.0) {
        return;
    }
    let border = Border::sides(
        BorderSide::new(widths[0], colors[0], RenderBorderStyle::Solid),
        BorderSide::new(widths[1], colors[1], RenderBorderStyle::Solid),
        BorderSide::new(widths[2], colors[2], RenderBorderStyle::Solid),
        BorderSide::new(widths[3], colors[3], RenderBorderStyle::Solid),
    );
    let style = BoxStyle {
        corner_radius: CornerRadius::all(3.0 * scale_factor),
        border: Some(border),
        ..BoxStyle::default()
    };
    canvas.draw_overlay_box_with_clips(bounds, style, clips.iter().copied());
}

fn paint_select_indicator(
    bounds: Rect,
    color: Color,
    scale_factor: f32,
    clips: &[Clip],
    canvas: &mut Canvas,
) {
    let font_size = 12.0 * scale_factor;
    let line_height = 14.0 * scale_factor;
    let indicator = Rect::new(
        bounds.x + (bounds.width - 19.0 * scale_factor).max(0.0),
        bounds.y + ((bounds.height - line_height) * 0.5).max(0.0),
        14.0 * scale_factor,
        line_height,
    );
    canvas.draw_text_with_clips(
        indicator,
        "▾",
        TextStyle {
            color,
            font_size,
            line_height,
            ..TextStyle::default()
        },
        clips.iter().copied(),
    );
}

fn paint_scrollbars(layout: &Layout, viewport: Rect, scale_factor: f32, canvas: &mut Canvas) {
    let track_style = BoxStyle {
        background: Color::from_rgba8(15, 23, 42, 36),
        corner_radius: CornerRadius::all(4.0 * scale_factor),
        ..BoxStyle::default()
    };
    let thumb_style = BoxStyle {
        background: Color::from_rgba8(71, 85, 105, 210),
        corner_radius: CornerRadius::all(4.0 * scale_factor),
        ..BoxStyle::default()
    };

    for layout in layout {
        let Some(scroll) = layout.scroll else {
            continue;
        };
        let clips = layout
            .clips
            .iter()
            .copied()
            .chain([scroll.clip])
            .collect::<Vec<_>>();
        for scrollbar in [scroll.horizontal, scroll.vertical].into_iter().flatten() {
            if !intersects_visible_area(scrollbar.track, &clips, viewport) {
                continue;
            }
            canvas.draw_overlay_box_with_clips(
                scaled_rect(scrollbar.track, scale_factor),
                track_style.clone(),
                clips
                    .iter()
                    .copied()
                    .map(|clip| scaled_clip(clip, scale_factor)),
            );
            canvas.draw_overlay_box_with_clips(
                scaled_rect(scrollbar.thumb, scale_factor),
                thumb_style.clone(),
                clips
                    .iter()
                    .copied()
                    .map(|clip| scaled_clip(clip, scale_factor)),
            );
        }
    }
}

fn visual_bounds(layout: &Layout) -> Rect {
    let mut bounds = Rect::new(layout.x, layout.y, layout.width, layout.height);
    match &layout.kind {
        LayoutKind::Box { style, .. } => {
            if let Some(outline) = style.outline {
                let expansion = (outline.offset + outline.width).max(0.0);
                bounds = expanded_rect(bounds, expansion);
            }
            if let Some(shadow) = style.shadow {
                let mut shadow_bounds =
                    expanded_rect(bounds, shadow.spread.max(0.0) + shadow.blur * 2.0);
                shadow_bounds.x += shadow.offset[0];
                shadow_bounds.y += shadow.offset[1];
                bounds = union_rect(bounds, shadow_bounds);
            }
            bounds = transformed_rect(bounds, style.transform.matrix);
        }
        LayoutKind::Text { style, .. } => {
            if let Some(shadow) = style.shadow {
                let mut shadow_bounds = expanded_rect(bounds, shadow.blur);
                shadow_bounds.x += shadow.offset[0];
                shadow_bounds.y += shadow.offset[1];
                bounds = union_rect(bounds, shadow_bounds);
            }
            bounds = transformed_rect(bounds, style.transform.matrix);
        }
    }
    bounds
}

fn expanded_rect(rect: Rect, amount: f32) -> Rect {
    Rect::new(
        rect.x - amount,
        rect.y - amount,
        rect.width + amount * 2.0,
        rect.height + amount * 2.0,
    )
}

fn union_rect(left: Rect, right: Rect) -> Rect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Rect::new(
        x,
        y,
        (left.x + left.width).max(right.x + right.width) - x,
        (left.y + left.height).max(right.y + right.height) - y,
    )
}

fn transformed_rect(rect: Rect, matrix: [f32; 6]) -> Rect {
    let [a, b, c, d, e, f] = matrix;
    let center = [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5];
    let corners = [
        [-rect.width * 0.5, -rect.height * 0.5],
        [rect.width * 0.5, -rect.height * 0.5],
        [-rect.width * 0.5, rect.height * 0.5],
        [rect.width * 0.5, rect.height * 0.5],
    ];
    let transformed = corners.map(|point| {
        [
            center[0] + a * point[0] + c * point[1] + e,
            center[1] + b * point[0] + d * point[1] + f,
        ]
    });
    let min_x = transformed
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = transformed
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = transformed
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = transformed
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn intersects_visible_area(mut bounds: Rect, clips: &[Clip], viewport: Rect) -> bool {
    bounds = bounds.intersection(viewport);
    for clip in clips {
        bounds = bounds.intersection(clip.rect);
    }
    bounds.width > 0.0 && bounds.height > 0.0
}

fn scaled_rect(rect: Rect, scale_factor: f32) -> Rect {
    Rect::new(
        rect.x * scale_factor,
        rect.y * scale_factor,
        rect.width * scale_factor,
        rect.height * scale_factor,
    )
}

fn scaled_clip(clip: Clip, scale_factor: f32) -> Clip {
    Clip::new(
        scaled_rect(clip.rect, scale_factor),
        CornerRadius::elliptical(
            scaled_corner(clip.corner_radius.top_left, scale_factor),
            scaled_corner(clip.corner_radius.top_right, scale_factor),
            scaled_corner(clip.corner_radius.bottom_right, scale_factor),
            scaled_corner(clip.corner_radius.bottom_left, scale_factor),
        ),
    )
}

fn scaled_box_style(style: BoxStyle, scale_factor: f32) -> BoxStyle {
    BoxStyle {
        background: style.background,
        background_image: style.background_image,
        corner_radius: CornerRadius::elliptical(
            scaled_corner(style.corner_radius.top_left, scale_factor),
            scaled_corner(style.corner_radius.top_right, scale_factor),
            scaled_corner(style.corner_radius.bottom_right, scale_factor),
            scaled_corner(style.corner_radius.bottom_left, scale_factor),
        ),
        border: style.border.map(|border| {
            Border::sides(
                scaled_border_side(border.top, scale_factor),
                scaled_border_side(border.right, scale_factor),
                scaled_border_side(border.bottom, scale_factor),
                scaled_border_side(border.left, scale_factor),
            )
        }),
        outline: style.outline.map(|outline| {
            Outline::new(
                outline.width * scale_factor,
                outline.offset * scale_factor,
                outline.color,
            )
        }),
        opacity: style.opacity,
        transform: Transform {
            matrix: [
                style.transform.matrix[0],
                style.transform.matrix[1],
                style.transform.matrix[2],
                style.transform.matrix[3],
                style.transform.matrix[4] * scale_factor,
                style.transform.matrix[5] * scale_factor,
            ],
        },
        shadow: style.shadow.map(|shadow| BoxShadow {
            offset: [
                shadow.offset[0] * scale_factor,
                shadow.offset[1] * scale_factor,
            ],
            blur: shadow.blur * scale_factor,
            spread: shadow.spread * scale_factor,
            color: shadow.color,
        }),
    }
}

fn scaled_corner(corner: CornerSize, scale_factor: f32) -> CornerSize {
    CornerSize::new(corner.x * scale_factor, corner.y * scale_factor)
}

fn scaled_border_side(side: BorderSide, scale_factor: f32) -> BorderSide {
    BorderSide::new(side.width * scale_factor, side.color, side.style)
}

fn scaled_text_style(style: &TextStyle, scale_factor: f32) -> TextStyle {
    let mut style = style.clone();
    style.font_size *= scale_factor;
    style.line_height *= scale_factor;
    style.letter_spacing *= scale_factor;
    style.word_spacing *= scale_factor;
    style.transform.matrix[4] *= scale_factor;
    style.transform.matrix[5] *= scale_factor;
    style.shadow = style.shadow.map(|shadow| TextShadow {
        offset: [
            shadow.offset[0] * scale_factor,
            shadow.offset[1] * scale_factor,
        ],
        blur: shadow.blur * scale_factor,
        color: shadow.color,
    });
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::{ElementKind, BODY_ID};
    use render::{BackgroundImage, DrawCommand};

    #[test]
    fn builds_canvas_from_computed_ui_layout() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        let text = document.create_node(ElementKind::Text("Hello UI".into()));
        document.set_style(card, "width", Some("300px")).unwrap();
        document
            .set_style(card, "background-color", Some("#f5f7fa"))
            .unwrap();
        document.set_style(card, "color", Some("#102030")).unwrap();
        document.insert(BODY_ID, card, None).unwrap();
        document.insert(card, text, None).unwrap();

        let canvas = build_canvas(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());

        assert_eq!(canvas.commands().len(), 2);
    }

    #[test]
    fn paints_a_native_select_indicator() {
        let mut document = Document::new();
        let select = document.create_node(ElementKind::Select);
        let option = document.create_node(ElementKind::Option);
        let text = document.create_node(ElementKind::Text("Choice".into()));
        document.insert(BODY_ID, select, None).unwrap();
        document.insert(select, option, None).unwrap();
        document.insert(option, text, None).unwrap();

        let canvas = build_canvas(&document, 300.0, 100.0, 1.0, &mut TextSystem::new());

        assert!(canvas.commands().iter().any(
            |command| matches!(command, render::DrawCommand::Text { text, .. } if text == "▾")
        ));
    }

    #[test]
    fn native_button_appearance_paints_borders_and_interaction_states() {
        let mut document = Document::new();
        let button = document.create_node(ElementKind::Button);
        document
            .set_attribute(button, "data-burokku-focused", Some(""))
            .unwrap();
        document
            .set_attribute(button, "aria-pressed", Some("true"))
            .unwrap();
        document.insert(BODY_ID, button, None).unwrap();

        let canvas = build_canvas(&document, 300.0, 100.0, 1.0, &mut TextSystem::new());
        let native_overlays = canvas
            .commands()
            .iter()
            .filter(|command| matches!(command, render::DrawCommand::OverlayBox { .. }))
            .count();
        assert_eq!(native_overlays, 2);
        assert!(canvas.commands().iter().any(|command| {
            matches!(
                command,
                render::DrawCommand::OverlayBox { style, .. }
                    if style.outline.is_some() && style.background.alpha > 0.0
            )
        }));
    }

    #[test]
    fn multiple_selects_do_not_paint_a_closed_dropdown_indicator() {
        let mut document = Document::new();
        let select = document.create_node(ElementKind::Select);
        let option = document.create_node(ElementKind::Option);
        let text = document.create_node(ElementKind::Text("Choice".into()));
        document
            .set_attribute(select, "multiple", Some(""))
            .unwrap();
        document.insert(BODY_ID, select, None).unwrap();
        document.insert(select, option, None).unwrap();
        document.insert(option, text, None).unwrap();

        let canvas = build_canvas(&document, 300.0, 100.0, 1.0, &mut TextSystem::new());

        assert!(!canvas.commands().iter().any(
            |command| matches!(command, render::DrawCommand::Text { text, .. } if text == "▾")
        ));
    }

    #[test]
    fn frame_preserves_the_layout_used_to_build_the_canvas() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("100px")).unwrap();
        document.set_style(card, "height", Some("50px")).unwrap();
        document
            .set_style(card, "background-color", Some("#ffffff"))
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let frame = build_frame(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());

        assert_eq!(frame.layout.kind.children()[0].element_id(), card);
        assert_eq!(frame.canvas.commands().len(), 1);
    }

    #[test]
    fn emits_overlay_shapes_for_text_decorations() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let text = document.create_node(ElementKind::Text("decorated".into()));
        document
            .set_style(
                container,
                "text-decoration",
                Some("underline line-through red"),
            )
            .unwrap();
        document
            .set_style(container, "text-align", Some("right"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, text, None).unwrap();

        let frame = build_frame(&document, 200.0, 100.0, 1.0, &mut TextSystem::new());
        let canvas = &frame.canvas;
        assert!(matches!(
            canvas.commands()[0],
            render::DrawCommand::Text { .. }
        ));
        let decorations = canvas
            .commands()
            .iter()
            .filter(|command| matches!(command, render::DrawCommand::OverlayBox { .. }))
            .count();
        assert_eq!(decorations, 2);
        let LayoutKind::Text { runs, .. } = &frame.layout.children()[0].children()[0].kind else {
            panic!("child should be text");
        };
        let render::DrawCommand::OverlayBox { rect, .. } = &canvas.commands()[1] else {
            panic!("decoration should be an overlay");
        };
        assert!((rect.x - runs[0].left).abs() < 0.01);
        assert!((rect.width - runs[0].width).abs() < 0.01);
        assert!(rect.x > 0.0);
        assert!(rect.width < 200.0);
    }

    #[test]
    fn wrapped_centered_decorations_follow_each_shaped_line() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let text =
            document.create_node(ElementKind::Text("decorations follow wrapped lines".into()));
        document
            .set_style(container, "width", Some("90px"))
            .unwrap();
        document
            .set_style(container, "text-align", Some("center"))
            .unwrap();
        document
            .set_style(container, "text-decoration", Some("underline"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, text, None).unwrap();

        let frame = build_frame(&document, 200.0, 150.0, 1.0, &mut TextSystem::new());
        let text_layout = &frame.layout.children()[0].children()[0];
        let LayoutKind::Text { runs, .. } = &text_layout.kind else {
            panic!("child should be text");
        };
        let decorations: Vec<_> = frame
            .canvas
            .commands()
            .iter()
            .filter_map(|command| match command {
                render::DrawCommand::OverlayBox { rect, .. } => Some(rect),
                _ => None,
            })
            .collect();

        assert!(runs.len() > 1);
        assert_eq!(decorations.len(), runs.len());
        for (rect, run) in decorations.into_iter().zip(runs) {
            assert!((rect.x - (text_layout.x + run.left)).abs() < 0.01);
            assert!((rect.width - run.width).abs() < 0.01);
            assert!(rect.width < text_layout.width);
        }
    }

    #[test]
    fn carries_paint_effects_from_style_to_canvas_commands() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("100px")).unwrap();
        document.set_style(card, "height", Some("50px")).unwrap();
        document.set_style(card, "opacity", Some("0.5")).unwrap();
        document
            .set_style(card, "transform", Some("translate(3px, 4px)"))
            .unwrap();
        document
            .set_style(card, "box-shadow", Some("1px 2px 3px 4px navy"))
            .unwrap();
        document
            .set_style(
                card,
                "background-image",
                Some("linear-gradient(to right, red, blue)"),
            )
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let canvas = build_canvas(&document, 200.0, 100.0, 2.0, &mut TextSystem::new());
        let DrawCommand::Box { style, .. } = &canvas.commands()[0] else {
            panic!("expected a box command");
        };
        assert_eq!(style.opacity, 0.5);
        assert_eq!(style.transform.matrix[4..], [6.0, 8.0]);
        assert_eq!(style.shadow.unwrap().offset, [2.0, 4.0]);
        assert!(matches!(
            style.background_image,
            Some(BackgroundImage::LinearGradient { .. })
        ));
    }

    #[test]
    fn carries_decoded_raster_backgrounds_to_canvas_commands() {
        const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("20px")).unwrap();
        document.set_style(card, "height", Some("10px")).unwrap();
        document
            .set_style(card, "background-image", Some(&format!("url({PNG})")))
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let canvas = build_canvas(&document, 100.0, 50.0, 1.0, &mut TextSystem::new());
        let DrawCommand::Box { style, .. } = &canvas.commands()[0] else {
            panic!("expected a box command");
        };
        let Some(BackgroundImage::Raster(image)) = &style.background_image else {
            panic!("expected raster background");
        };
        assert_eq!((image.width, image.height), (2, 1));
    }

    #[test]
    fn scales_geometry_and_paint_styles_for_physical_pixels() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("100px")).unwrap();
        document.set_style(card, "height", Some("50px")).unwrap();
        document
            .set_style(card, "border-width", Some("1px 2px 3px 4px"))
            .unwrap();
        document
            .set_style(card, "border-style", Some("solid"))
            .unwrap();
        document
            .set_style(card, "border-radius", Some("4px"))
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let canvas = build_canvas(&document, 800.0, 600.0, 2.0, &mut TextSystem::new());
        let render::DrawCommand::Box { rect, style, .. } = &canvas.commands()[0] else {
            panic!("card should produce a box command");
        };

        assert_eq!((rect.width, rect.height), (212.0, 108.0));
        assert_eq!(style.border.expect("border").widths(), [2.0, 4.0, 6.0, 8.0]);
        assert_eq!(style.corner_radius.top_left, CornerSize::all(8.0));
    }

    #[test]
    fn canvas_commands_follow_stacking_layer_order() {
        let mut document = Document::new();
        let ordinary = document.create_node(ElementKind::Div);
        let high_descendant = document.create_node(ElementKind::Div);
        let middle = document.create_node(ElementKind::Div);
        let negative = document.create_node(ElementKind::Div);

        for (element, color) in [
            (ordinary, "#ff0000"),
            (high_descendant, "#00ff00"),
            (middle, "#0000ff"),
            (negative, "#000000"),
        ] {
            document
                .set_style(element, "background-color", Some(color))
                .unwrap();
            document.set_style(element, "width", Some("20px")).unwrap();
            document.set_style(element, "height", Some("20px")).unwrap();
        }
        document
            .set_style(high_descendant, "z-index", Some("10"))
            .unwrap();
        document.set_style(middle, "z-index", Some("5")).unwrap();
        document.set_style(negative, "z-index", Some("-1")).unwrap();

        document.insert(BODY_ID, ordinary, None).unwrap();
        document.insert(ordinary, high_descendant, None).unwrap();
        document.insert(BODY_ID, middle, None).unwrap();
        document.insert(BODY_ID, negative, None).unwrap();

        let canvas = build_canvas(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
        let backgrounds: Vec<_> = canvas
            .commands()
            .iter()
            .filter_map(|command| match command {
                render::DrawCommand::Box { style, .. } => Some(style.background),
                render::DrawCommand::OverlayBox { .. } | render::DrawCommand::Text { .. } => None,
            })
            .collect();

        assert_eq!(
            backgrounds,
            [
                Color::BLACK,
                Color::from_rgba8(0xff, 0, 0, 0xff),
                Color::from_rgba8(0, 0, 0xff, 0xff),
                Color::from_rgba8(0, 0xff, 0, 0xff),
            ]
        );
    }

    #[test]
    fn hidden_and_clip_overflow_clip_descendants_to_the_padding_box() {
        for overflow in ["hidden", "clip"] {
            let mut document = Document::new();
            let container = document.create_node(ElementKind::Div);
            let overflowing = document.create_node(ElementKind::Div);
            document
                .set_style(container, "width", Some("100px"))
                .unwrap();
            document
                .set_style(container, "height", Some("40px"))
                .unwrap();
            document
                .set_style(container, "border-width", Some("2px"))
                .unwrap();
            document
                .set_style(container, "border-style", Some("solid"))
                .unwrap();
            document
                .set_style(container, "border-radius", Some("12px"))
                .unwrap();
            document
                .set_style(container, "overflow", Some(overflow))
                .unwrap();
            document
                .set_style(overflowing, "width", Some("180px"))
                .unwrap();
            document
                .set_style(overflowing, "height", Some("80px"))
                .unwrap();
            document
                .set_style(overflowing, "background-color", Some("#ff0000"))
                .unwrap();
            document
                .set_style(overflowing, "z-index", Some("10"))
                .unwrap();
            document.insert(BODY_ID, container, None).unwrap();
            document.insert(container, overflowing, None).unwrap();

            let frame = build_frame(&document, 300.0, 200.0, 2.0, &mut TextSystem::new());
            let container_layout = &frame.layout.children()[0];
            let overflowing_layout = &container_layout.children()[0];
            let expected_clip = Rect::new(
                container_layout.x + 2.0,
                container_layout.y + 2.0,
                container_layout.width - 4.0,
                container_layout.height - 4.0,
            );

            let expected_clip = Clip::new(expected_clip, CornerRadius::all(10.0));
            assert_eq!(overflowing_layout.clips, [expected_clip]);
            let overflowing_command = frame
                .canvas
                .commands()
                .iter()
                .find(|command| {
                    matches!(
                        command,
                        render::DrawCommand::Box { style, .. }
                            if style.background == Color::from_rgba8(0xff, 0, 0, 0xff)
                    )
                })
                .expect("overflowing child box");
            let render::DrawCommand::Box { clips, .. } = overflowing_command else {
                unreachable!()
            };
            assert_eq!(*clips, [scaled_clip(expected_clip, 2.0)]);

            let outside_container = (
                container_layout.x + container_layout.width + 10.0,
                container_layout.y + 10.0,
            );
            assert_ne!(
                frame
                    .layout
                    .hit_test(outside_container.0, outside_container.1)
                    .map(Layout::element_id),
                Some(overflowing)
            );
            let rounded_corner = (expected_clip.rect.x + 1.0, expected_clip.rect.y + 1.0);
            assert_ne!(
                frame
                    .layout
                    .hit_test(rounded_corner.0, rounded_corner.1)
                    .map(Layout::element_id),
                Some(overflowing)
            );
        }
    }

    #[test]
    fn overflow_axis_only_clips_that_axis() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let overflowing = document.create_node(ElementKind::Div);
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("40px"))
            .unwrap();
        document
            .set_style(container, "overflow-x", Some("hidden"))
            .unwrap();
        document
            .set_style(overflowing, "width", Some("180px"))
            .unwrap();
        document
            .set_style(overflowing, "height", Some("80px"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, overflowing, None).unwrap();

        let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let container_layout = &layout.children()[0];
        let clip = container_layout.children()[0].clips[0].rect;

        assert_eq!(clip.x, container_layout.x);
        assert_eq!(clip.width, container_layout.width);
        assert_eq!((clip.y, clip.height), (0.0, 200.0));
    }

    #[test]
    fn scroll_overflow_offsets_content_and_builds_proportional_scrollbars() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("60px"))
            .unwrap();
        document
            .set_style(container, "overflow", Some("auto"))
            .unwrap();
        document.set_style(content, "width", Some("240px")).unwrap();
        document
            .set_style(content, "height", Some("180px"))
            .unwrap();
        document
            .set_style(content, "background-color", Some("#ff0000"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, content, None).unwrap();

        let offsets = HashMap::from([(container, ScrollOffset::new(50.0, 40.0))]);
        let frame = build_frame_with_scroll(
            &document,
            300.0,
            200.0,
            1.0,
            &mut TextSystem::new(),
            &offsets,
        );
        let container_layout = &frame.layout.children()[0];
        let content_layout = &container_layout.children()[0];
        let scroll = container_layout.scroll.expect("scroll container");

        assert_eq!(scroll.max_offset, ScrollOffset::new(140.0, 120.0));
        assert_eq!(scroll.offset, ScrollOffset::new(50.0, 40.0));
        assert_eq!(
            (content_layout.x, content_layout.y),
            (container_layout.x - 50.0, container_layout.y - 40.0)
        );
        let horizontal = scroll.horizontal.expect("horizontal scrollbar");
        let vertical = scroll.vertical.expect("vertical scrollbar");
        assert!(horizontal.thumb.width < horizontal.track.width);
        assert!(vertical.thumb.height < vertical.track.height);
        assert!(horizontal.thumb.x > horizontal.track.x);
        assert!(vertical.thumb.y > vertical.track.y);

        let scrollbar_boxes = frame
            .canvas
            .commands()
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    render::DrawCommand::OverlayBox { style, .. }
                        if style.background == Color::from_rgba8(15, 23, 42, 36)
                            || style.background == Color::from_rgba8(71, 85, 105, 210)
                )
            })
            .count();
        assert_eq!(scrollbar_boxes, 4);
    }

    #[test]
    fn scroll_offsets_are_clamped_when_content_shrinks() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("60px"))
            .unwrap();
        document
            .set_style(container, "overflow", Some("auto"))
            .unwrap();
        document.set_style(content, "width", Some("120px")).unwrap();
        document.set_style(content, "height", Some("80px")).unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, content, None).unwrap();

        let offsets = HashMap::from([(container, ScrollOffset::new(500.0, 500.0))]);
        let frame = build_frame_with_scroll(
            &document,
            300.0,
            200.0,
            1.0,
            &mut TextSystem::new(),
            &offsets,
        );
        let scroll = frame.layout.children()[0].scroll.expect("scroll container");

        assert_eq!(scroll.max_offset, ScrollOffset::new(20.0, 20.0));
        assert_eq!(scroll.offset, scroll.max_offset);
    }

    #[test]
    fn scroll_always_shows_tracks_while_auto_only_shows_them_for_overflow() {
        let mut document = Document::new();
        let automatic = document.create_node(ElementKind::Div);
        let forced = document.create_node(ElementKind::Div);
        for (element, overflow) in [(automatic, "auto"), (forced, "scroll")] {
            document.set_style(element, "width", Some("100px")).unwrap();
            document.set_style(element, "height", Some("60px")).unwrap();
            document
                .set_style(element, "overflow", Some(overflow))
                .unwrap();
            document.insert(BODY_ID, element, None).unwrap();
        }

        let frame = build_frame(&document, 300.0, 200.0, 1.0, &mut TextSystem::new());
        let automatic = frame.layout.children()[0].scroll.expect("auto container");
        let forced = frame.layout.children()[1].scroll.expect("scroll container");

        assert!(automatic.horizontal.is_none() && automatic.vertical.is_none());
        assert!(forced.horizontal.is_some() && forced.vertical.is_some());
    }

    #[test]
    fn retained_scroll_update_matches_a_full_layout_rebuild() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Div);
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("60px"))
            .unwrap();
        document
            .set_style(container, "overflow", Some("auto"))
            .unwrap();
        document.set_style(content, "width", Some("240px")).unwrap();
        document
            .set_style(content, "height", Some("180px"))
            .unwrap();
        document
            .set_style(content, "overflow", Some("auto"))
            .unwrap();
        document.set_style(child, "width", Some("400px")).unwrap();
        document.set_style(child, "height", Some("300px")).unwrap();
        document
            .set_style(child, "background-color", Some("#ff0000"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, content, None).unwrap();
        document.insert(content, child, None).unwrap();

        let mut retained = build_frame(&document, 300.0, 200.0, 2.0, &mut TextSystem::new());
        let outer_offset = ScrollOffset::new(50.0, 40.0);
        let inner_offset = ScrollOffset::new(60.0, 50.0);
        assert!(retained.layout.apply_scroll_offset(container, outer_offset));
        assert!(retained.layout.apply_scroll_offset(content, inner_offset));
        repaint_frame(&mut retained, 2.0);

        let expected = build_frame_with_scroll(
            &document,
            300.0,
            200.0,
            2.0,
            &mut TextSystem::new(),
            &HashMap::from([(container, outer_offset), (content, inner_offset)]),
        );
        assert_eq!(retained, expected);
    }

    #[test]
    fn canvas_culls_boxes_outside_the_scroll_clip() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        document
            .set_style(container, "display", Some("flex"))
            .unwrap();
        document
            .set_style(container, "flex-direction", Some("column"))
            .unwrap();
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("60px"))
            .unwrap();
        document
            .set_style(container, "overflow", Some("auto"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();

        for color in ["#ff0000", "#00ff00", "#0000ff", "#000000"] {
            let child = document.create_node(ElementKind::Div);
            document.set_style(child, "width", Some("100px")).unwrap();
            document.set_style(child, "height", Some("40px")).unwrap();
            document.set_style(child, "flex-shrink", Some("0")).unwrap();
            document
                .set_style(child, "background-color", Some(color))
                .unwrap();
            document.insert(container, child, None).unwrap();
        }

        let frame = build_frame_with_scroll(
            &document,
            300.0,
            200.0,
            1.0,
            &mut TextSystem::new(),
            &HashMap::from([(container, ScrollOffset::new(0.0, 80.0))]),
        );
        let backgrounds: Vec<_> = frame
            .canvas
            .commands()
            .iter()
            .filter_map(|command| match command {
                render::DrawCommand::Box { style, .. } => Some(style.background),
                _ => None,
            })
            .collect();

        assert!(!backgrounds.contains(&Color::from_rgba8(0xff, 0, 0, 0xff)));
        assert!(!backgrounds.contains(&Color::from_rgba8(0, 0xff, 0, 0xff)));
        assert!(backgrounds.contains(&Color::from_rgba8(0, 0, 0xff, 0xff)));
    }
}
