use std::collections::HashMap;

use render::{
    Border, BoxShadow, BoxStyle, Canvas, Clip, Color, CornerRadius, Outline, Rect,
    TextDecorationLine, TextRunMetrics, TextShadow, TextSpan, TextStyle, TextSystem, Transform,
};

use super::{
    elements::Document,
    layouts::{compute_layout, compute_layout_with_scroll, Layout, LayoutKind, ScrollOffset},
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
    if establishes_effect_group(layout) {
        paint_group(
            layout,
            viewport,
            scale_factor,
            0,
            Transform::IDENTITY,
            canvas,
        );
    } else {
        paint_context(
            layout,
            viewport,
            scale_factor,
            0,
            false,
            Transform::IDENTITY,
            canvas,
        );
    }
}

fn paint_context(
    root: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    strip_root_effect: bool,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    let flattened = root.iter().collect::<Vec<_>>();
    let mut index = 0;
    while index < flattened.len() {
        let layout = flattened[index];
        if layout.element_id != root.element_id && establishes_effect_group(layout) {
            paint_group(
                layout,
                viewport,
                scale_factor,
                clip_skip,
                context_world,
                canvas,
            );
            index += layout.iter().count();
        } else {
            paint_one(
                layout,
                viewport,
                scale_factor,
                clip_skip,
                strip_root_effect && layout.element_id == root.element_id,
                context_world,
                canvas,
            );
            index += 1;
        }
    }
}

fn paint_group(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    parent_context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut group = Canvas::new();
    paint_context(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        true,
        layout_world_transform(layout),
        &mut group,
    );
    let (opacity, transform) = layout_effect(layout);
    canvas.draw_group(
        group,
        [
            (layout.x + layout.width * 0.5) * scale_factor,
            (layout.y + layout.height * 0.5) * scale_factor,
        ],
        scaled_transform(transform, scale_factor),
        opacity,
        layout
            .clips
            .iter()
            .skip(clip_skip)
            .copied()
            .map(|clip| localize_clip(clip, parent_context_world))
            .map(|clip| scaled_clip(clip, scale_factor)),
    );
}

fn paint_one(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    strip_effect: bool,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    if layout.width <= 0.0 && layout.height <= 0.0 {
        return;
    }
    let bounds = Rect::new(
        layout.x * scale_factor,
        layout.y * scale_factor,
        layout.width * scale_factor,
        layout.height * scale_factor,
    );
    if !intersects_visible_area(visual_bounds(layout), &layout.clips, viewport) {
        return;
    }
    let clips = layout
        .clips
        .iter()
        .skip(clip_skip)
        .copied()
        .map(|clip| localize_clip(clip, context_world))
        .map(|clip| scaled_clip(clip, scale_factor))
        .collect::<Vec<_>>();
    match &layout.kind {
        LayoutKind::Box { style, .. } => {
            let mut style = style.clone();
            if strip_effect {
                style.opacity = 1.0;
                style.transform = Transform::IDENTITY;
            }
            if style != BoxStyle::default() {
                canvas.draw_box_with_clips(
                    bounds,
                    scaled_box_style(style, scale_factor),
                    clips.iter().copied(),
                );
            }
        }
        LayoutKind::Text {
            spans, style, runs, ..
        } => {
            let mut style = style.clone();
            if strip_effect {
                style.opacity = 1.0;
                style.transform = Transform::IDENTITY;
            }
            let scaled_style = scaled_text_style(&style, scale_factor);
            let scaled_spans = scaled_text_spans(spans, scale_factor);
            canvas.draw_rich_text_with_clips(
                bounds,
                scaled_spans.clone(),
                scaled_style,
                clips.iter().copied(),
            );
            paint_text_decorations(
                bounds,
                &scaled_spans,
                runs,
                scale_factor,
                clips.iter().copied(),
                canvas,
            );
        }
    }
}

fn paint_text_decorations(
    bounds: Rect,
    spans: &[TextSpan],
    runs: &[TextRunMetrics],
    scale_factor: f32,
    clips: impl IntoIterator<Item = Clip> + Clone,
    canvas: &mut Canvas,
) {
    for run in runs {
        let Some(style) = spans.get(run.span_index).map(|span| &span.style) else {
            continue;
        };
        if style.text_decoration_line == TextDecorationLine::NONE {
            continue;
        }
        let decoration_style = BoxStyle {
            background: style.text_decoration_color,
            ..BoxStyle::default()
        };
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

fn establishes_effect_group(layout: &Layout) -> bool {
    let (opacity, transform) = layout_effect(layout);
    opacity < 1.0 || transform != Transform::IDENTITY
}

fn layout_effect(layout: &Layout) -> (f32, Transform) {
    match &layout.kind {
        LayoutKind::Box { style, .. } => (style.opacity, style.transform),
        LayoutKind::Text { style, .. } => (style.opacity, style.transform),
    }
}

fn scaled_transform(transform: Transform, scale_factor: f32) -> Transform {
    let mut transform = transform;
    transform.matrix[4] *= scale_factor;
    transform.matrix[5] *= scale_factor;
    transform
}

fn layout_world_transform(layout: &Layout) -> Transform {
    let center = [
        layout.x + layout.width * 0.5,
        layout.y + layout.height * 0.5,
    ];
    anchored_transform(layout.transform, center)
}

fn localize_clip(mut clip: Clip, context_world: Transform) -> Clip {
    if context_world == Transform::IDENTITY {
        return clip;
    }
    let center = [
        clip.rect.x + clip.rect.width * 0.5,
        clip.rect.y + clip.rect.height * 0.5,
    ];
    let clip_world = anchored_transform(
        Transform {
            matrix: clip.transform,
        },
        center,
    );
    let Some(context_inverse) = inverse_transform(context_world) else {
        return clip;
    };
    let localized = multiply_transform(context_inverse, clip_world);
    clip.transform = relative_transform(localized, center).matrix;
    clip
}

fn anchored_transform(transform: Transform, center: [f32; 2]) -> Transform {
    let [a, b, c, d, tx, ty] = transform.matrix;
    Transform {
        matrix: [
            a,
            b,
            c,
            d,
            center[0] + tx - a * center[0] - c * center[1],
            center[1] + ty - b * center[0] - d * center[1],
        ],
    }
}

fn relative_transform(transform: Transform, center: [f32; 2]) -> Transform {
    let [a, b, c, d, tx, ty] = transform.matrix;
    Transform {
        matrix: [
            a,
            b,
            c,
            d,
            tx - center[0] + a * center[0] + c * center[1],
            ty - center[1] + b * center[0] + d * center[1],
        ],
    }
}

fn multiply_transform(left: Transform, right: Transform) -> Transform {
    let [la, lb, lc, ld, ltx, lty] = left.matrix;
    let [ra, rb, rc, rd, rtx, rty] = right.matrix;
    Transform {
        matrix: [
            la * ra + lc * rb,
            lb * ra + ld * rb,
            la * rc + lc * rd,
            lb * rc + ld * rd,
            la * rtx + lc * rty + ltx,
            lb * rtx + ld * rty + lty,
        ],
    }
}

fn inverse_transform(transform: Transform) -> Option<Transform> {
    let [a, b, c, d, tx, ty] = transform.matrix;
    let determinant = a * d - b * c;
    if determinant.abs() <= f32::EPSILON {
        return None;
    }
    let inverse = [
        d / determinant,
        -b / determinant,
        -c / determinant,
        a / determinant,
    ];
    Some(Transform {
        matrix: [
            inverse[0],
            inverse[1],
            inverse[2],
            inverse[3],
            -inverse[0] * tx - inverse[2] * ty,
            -inverse[1] * tx - inverse[3] * ty,
        ],
    })
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
            for shadow in style.shadows.iter().filter(|shadow| !shadow.inset) {
                let mut shadow_bounds =
                    expanded_rect(bounds, shadow.spread.max(0.0) + shadow.blur * 2.0);
                shadow_bounds.x += shadow.offset[0];
                shadow_bounds.y += shadow.offset[1];
                bounds = union_rect(bounds, shadow_bounds);
            }
            bounds = transformed_rect(bounds, layout.transform.matrix);
        }
        LayoutKind::Text { style, .. } => {
            for shadow in &style.shadows {
                let mut shadow_bounds = expanded_rect(bounds, shadow.blur);
                shadow_bounds.x += shadow.offset[0];
                shadow_bounds.y += shadow.offset[1];
                bounds = union_rect(bounds, shadow_bounds);
            }
            bounds = transformed_rect(bounds, layout.transform.matrix);
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
        bounds = bounds.intersection(clip.bounds());
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
    let mut scaled = Clip::new(
        scaled_rect(clip.rect, scale_factor),
        CornerRadius::new(
            clip.corner_radius.top_left * scale_factor,
            clip.corner_radius.top_right * scale_factor,
            clip.corner_radius.bottom_right * scale_factor,
            clip.corner_radius.bottom_left * scale_factor,
        ),
    );
    scaled.transform = [
        clip.transform[0],
        clip.transform[1],
        clip.transform[2],
        clip.transform[3],
        clip.transform[4] * scale_factor,
        clip.transform[5] * scale_factor,
    ];
    scaled
}

fn scaled_box_style(style: BoxStyle, scale_factor: f32) -> BoxStyle {
    BoxStyle {
        background: style.background,
        background_image: style.background_image,
        corner_radius: CornerRadius::new(
            style.corner_radius.top_left * scale_factor,
            style.corner_radius.top_right * scale_factor,
            style.corner_radius.bottom_right * scale_factor,
            style.corner_radius.bottom_left * scale_factor,
        ),
        border: style
            .border
            .map(|border| Border::new(border.width * scale_factor, border.color)),
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
        shadows: style
            .shadows
            .into_iter()
            .map(|shadow| BoxShadow {
                offset: [
                    shadow.offset[0] * scale_factor,
                    shadow.offset[1] * scale_factor,
                ],
                blur: shadow.blur * scale_factor,
                spread: shadow.spread * scale_factor,
                color: shadow.color,
                inset: shadow.inset,
            })
            .collect(),
    }
}

fn scaled_text_style(style: &TextStyle, scale_factor: f32) -> TextStyle {
    let mut style = style.clone();
    style.font_size *= scale_factor;
    style.line_height *= scale_factor;
    style.letter_spacing *= scale_factor;
    style.word_spacing *= scale_factor;
    style.transform.matrix[4] *= scale_factor;
    style.transform.matrix[5] *= scale_factor;
    style.shadows = style
        .shadows
        .into_iter()
        .map(|shadow| TextShadow {
            offset: [
                shadow.offset[0] * scale_factor,
                shadow.offset[1] * scale_factor,
            ],
            blur: shadow.blur * scale_factor,
            color: shadow.color,
        })
        .collect();
    style
}

fn scaled_text_spans(spans: &[TextSpan], scale_factor: f32) -> Vec<TextSpan> {
    spans
        .iter()
        .map(|span| TextSpan::new(&span.text, scaled_text_style(&span.style, scale_factor)))
        .collect()
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
        let DrawCommand::Group {
            canvas,
            opacity,
            transform,
            ..
        } = &canvas.commands()[0]
        else {
            panic!("expected an effect group");
        };
        assert_eq!(*opacity, 0.5);
        assert_eq!(transform.matrix[4..], [6.0, 8.0]);
        let DrawCommand::Box { style, .. } = &canvas.commands()[0] else {
            panic!("expected a grouped box command");
        };
        assert_eq!(style.opacity, 1.0);
        assert_eq!(style.transform, Transform::IDENTITY);
        assert_eq!(style.shadows[0].offset, [2.0, 4.0]);
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
    fn retains_opacity_and_transform_subtrees_as_recursive_groups() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Text("rotated".into()));
        document.set_style(parent, "width", Some("100px")).unwrap();
        document.set_style(parent, "height", Some("80px")).unwrap();
        document.set_style(parent, "opacity", Some("50%")).unwrap();
        document
            .set_style(parent, "transform", Some("rotate(30deg) skewX(10deg)"))
            .unwrap();
        document.set_style(first, "width", Some("40px")).unwrap();
        document.set_style(first, "height", Some("30px")).unwrap();
        document
            .set_style(first, "background-color", Some("red"))
            .unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, first, None).unwrap();
        document.insert(parent, second, None).unwrap();

        let canvas = build_canvas(&document, 200.0, 160.0, 1.0, &mut TextSystem::new());
        let DrawCommand::Group {
            canvas,
            opacity,
            transform,
            origin,
            ..
        } = &canvas.commands()[0]
        else {
            panic!("expected recursive effect group");
        };
        assert_eq!(*opacity, 0.5);
        assert_ne!(*transform, Transform::IDENTITY);
        assert_eq!(*origin, [50.0, 40.0]);
        assert!(canvas
            .commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::Text { .. })));
        assert!(canvas
            .commands()
            .iter()
            .any(|command| matches!(command, DrawCommand::Box { .. })));
    }

    #[test]
    fn localizes_overflow_clips_inside_transformed_groups() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Div);
        document.set_style(parent, "width", Some("100px")).unwrap();
        document.set_style(parent, "height", Some("80px")).unwrap();
        document
            .set_style(parent, "overflow", Some("hidden"))
            .unwrap();
        document
            .set_style(parent, "transform", Some("rotate(30deg)"))
            .unwrap();
        document.set_style(child, "width", Some("120px")).unwrap();
        document.set_style(child, "height", Some("100px")).unwrap();
        document
            .set_style(child, "background-color", Some("red"))
            .unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        let canvas = build_canvas(&document, 200.0, 160.0, 1.0, &mut TextSystem::new());
        let DrawCommand::Group { canvas, .. } = &canvas.commands()[0] else {
            panic!("expected transformed parent group");
        };
        let child_clip = canvas
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Box { clips, .. } if !clips.is_empty() => Some(clips[0]),
                _ => None,
            })
            .expect("overflow clip on child");
        for (actual, expected) in child_clip
            .transform
            .into_iter()
            .zip(Transform::IDENTITY.matrix)
        {
            assert!((actual - expected).abs() < 0.0001);
        }
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
    fn decorates_only_the_styled_inline_span() {
        let mut document = Document::new();
        let line = document.create_node(ElementKind::Span);
        let before = document.create_node(ElementKind::Text("before ".into()));
        let decorated = document.create_node(ElementKind::Span);
        let decorated_text = document.create_node(ElementKind::Text("middle".into()));
        let after = document.create_node(ElementKind::Text(" after".into()));
        document
            .set_style(decorated, "text-decoration", Some("underline #7c3aed"))
            .unwrap();
        document.insert(BODY_ID, line, None).unwrap();
        document.insert(line, before, None).unwrap();
        document.insert(line, decorated, None).unwrap();
        document.insert(decorated, decorated_text, None).unwrap();
        document.insert(line, after, None).unwrap();

        let frame = build_frame(&document, 300.0, 100.0, 1.0, &mut TextSystem::new());
        let inline = &frame.layout.children()[0];
        let LayoutKind::Text { spans, runs, .. } = &inline.kind else {
            panic!("nested spans should produce one inline text layout");
        };
        let decorated_runs = runs.iter().filter(|run| run.span_index == 1).count();
        let decorations: Vec<_> = frame
            .canvas
            .commands()
            .iter()
            .filter_map(|command| match command {
                render::DrawCommand::OverlayBox { style, .. } => Some(style),
                _ => None,
            })
            .collect();

        assert_eq!(spans.len(), 3);
        assert!(decorated_runs > 0);
        assert_eq!(decorations.len(), decorated_runs);
        assert!(decorations
            .iter()
            .all(|style| style.background == Color::from_rgba8(0x7c, 0x3a, 0xed, 0xff)));
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
    fn scales_geometry_and_paint_styles_for_physical_pixels() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("100px")).unwrap();
        document.set_style(card, "height", Some("50px")).unwrap();
        document
            .set_style(card, "border-width", Some("2px"))
            .unwrap();
        document
            .set_style(card, "border-radius", Some("4px"))
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let canvas = build_canvas(&document, 800.0, 600.0, 2.0, &mut TextSystem::new());
        let render::DrawCommand::Box { rect, style, .. } = &canvas.commands()[0] else {
            panic!("card should produce a box command");
        };

        assert_eq!((rect.width, rect.height), (208.0, 108.0));
        assert_eq!(style.border.expect("border").width, 4.0);
        assert_eq!(style.corner_radius.top_left, 8.0);
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
                render::DrawCommand::OverlayBox { .. }
                | render::DrawCommand::Text { .. }
                | render::DrawCommand::Group { .. } => None,
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
