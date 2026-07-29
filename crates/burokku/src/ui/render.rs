use std::collections::HashMap;

use render::{
    Border, BoxDecoration, BoxShadow, BoxStyle, Canvas, Clip, Color, CornerRadius, DecorationStyle,
    Outline, PaintLayer, Rect, TextDecorationLine, TextRunMetrics, TextShadow, TextSpan, TextStyle,
    TextSystem, Transform,
};

use super::{
    elements::Document,
    layouts::{
        compute_layout, compute_layout_with_scroll, descendant_contexts, zero_level_entries,
        Layout, LayoutKind, ScrollOffset, Stacking, ZeroLevelEntry,
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
        paint_atomic_context(
            layout,
            viewport,
            scale_factor,
            0,
            PaintLayer::ContextBackground,
            Transform::IDENTITY,
            canvas,
        );
    } else {
        paint_stacking_context(
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

fn paint_stacking_context(
    root: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    strip_root_effect: bool,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    paint_phased_layout(
        root,
        PaintLayer::ContextBackground,
        viewport,
        scale_factor,
        clip_skip,
        strip_root_effect,
        context_world,
        canvas,
    );

    let contexts = descendant_contexts(root);
    for context in contexts
        .iter()
        .copied()
        .filter(|layout| layout.stacking_index() < 0)
    {
        paint_atomic_context(
            context,
            viewport,
            scale_factor,
            clip_skip,
            PaintLayer::Negative,
            context_world,
            canvas,
        );
    }

    paint_ordinary_descendants(
        root,
        viewport,
        scale_factor,
        clip_skip,
        context_world,
        canvas,
    );
    paint_context_scrollbars(
        root,
        viewport,
        scale_factor,
        clip_skip,
        context_world,
        canvas,
    );

    for entry in zero_level_entries(root) {
        match entry {
            ZeroLevelEntry::Context(context) => paint_atomic_context(
                context,
                viewport,
                scale_factor,
                clip_skip,
                positioned_layer(context),
                context_world,
                canvas,
            ),
            ZeroLevelEntry::PositionedAuto(layout) => paint_positioned_auto(
                layout,
                viewport,
                scale_factor,
                clip_skip,
                context_world,
                canvas,
            ),
        }
    }

    for context in contexts
        .iter()
        .copied()
        .filter(|layout| layout.stacking_index() > 0)
    {
        paint_atomic_context(
            context,
            viewport,
            scale_factor,
            clip_skip,
            positioned_layer(context),
            context_world,
            canvas,
        );
    }
}

fn positioned_layer(layout: &Layout) -> PaintLayer {
    if layout.is_fixed_to_viewport() {
        PaintLayer::Fixed
    } else {
        PaintLayer::Positioned
    }
}

fn paint_atomic_context(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    layer: PaintLayer,
    parent_context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut group = Canvas::new();
    let has_effect = establishes_effect_group(layout);
    let context_world = if has_effect {
        layout_world_transform(layout)
    } else {
        parent_context_world
    };
    paint_stacking_context(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        has_effect,
        context_world,
        &mut group,
    );
    if group.commands().is_empty() {
        return;
    }
    let (opacity, transform) = layout_effect(layout);
    canvas.draw_group_in_layer(
        layer,
        group,
        [
            (layout.x + layout.width * 0.5) * scale_factor,
            (layout.y + layout.height * 0.5) * scale_factor,
        ],
        scaled_transform(transform, scale_factor),
        if has_effect { opacity } else { 1.0 },
        layout
            .clips
            .iter()
            .skip(clip_skip)
            .copied()
            .map(|clip| localize_clip(clip, parent_context_world))
            .map(|clip| scaled_clip(clip, scale_factor)),
    );
}

fn paint_positioned_auto(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    parent_context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut group = Canvas::new();
    paint_phased_layout(
        layout,
        PaintLayer::ContextBackground,
        viewport,
        scale_factor,
        layout.clips.len(),
        false,
        parent_context_world,
        &mut group,
    );
    paint_ordinary_descendants(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        parent_context_world,
        &mut group,
    );
    paint_context_scrollbars(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        parent_context_world,
        &mut group,
    );
    if group.commands().is_empty() {
        return;
    }
    canvas.draw_group_in_layer(
        PaintLayer::Positioned,
        group,
        [
            (layout.x + layout.width * 0.5) * scale_factor,
            (layout.y + layout.height * 0.5) * scale_factor,
        ],
        Transform::IDENTITY,
        1.0,
        layout
            .clips
            .iter()
            .skip(clip_skip)
            .copied()
            .map(|clip| localize_clip(clip, parent_context_world))
            .map(|clip| scaled_clip(clip, scale_factor)),
    );
}

fn paint_ordinary_descendants(
    root: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut pending = vec![root.children().iter()];
    while let Some(mut children) = pending.pop() {
        let Some(layout) = children.next() else {
            continue;
        };
        pending.push(children);
        if layout.establishes_stacking_context() || layout.is_positioned_auto() {
            continue;
        }
        paint_phased_layout(
            layout,
            PaintLayer::Block,
            viewport,
            scale_factor,
            clip_skip,
            false,
            context_world,
            canvas,
        );
        pending.push(layout.children().iter());
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "painting one retained layout needs its phase and render context"
)]
fn paint_phased_layout(
    layout: &Layout,
    box_layer: PaintLayer,
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
                paint_box_decorations(
                    bounds,
                    scaled_box_style(style, scale_factor),
                    box_layer,
                    clips.iter().copied(),
                    canvas,
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

fn paint_box_decorations(
    bounds: Rect,
    style: BoxStyle,
    layer: PaintLayer,
    clips: impl IntoIterator<Item = Clip> + Clone,
    canvas: &mut Canvas,
) {
    let decoration_style = DecorationStyle {
        corner_radius: style.corner_radius,
        opacity: style.opacity,
        transform: style.transform,
    };
    for shadow in style.shadows.iter().rev().filter(|shadow| !shadow.inset) {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Shadow(*shadow),
            decoration_style,
            clips.clone(),
        );
    }
    if style.background != Color::TRANSPARENT || style.background_image.is_some() {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Background {
                color: style.background,
                image: style.background_image,
            },
            decoration_style,
            clips.clone(),
        );
    }
    for shadow in style.shadows.iter().rev().filter(|shadow| shadow.inset) {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Shadow(*shadow),
            decoration_style,
            clips.clone(),
        );
    }
    if let Some(border) = style.border {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Border(border),
            decoration_style,
            clips.clone(),
        );
    }
    if let Some(outline) = style.outline {
        canvas.draw_decoration_with_clips(
            PaintLayer::Outline,
            bounds,
            BoxDecoration::Outline(outline),
            decoration_style,
            clips,
        );
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

fn paint_context_scrollbars(
    root: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut pending = vec![root];
    while let Some(layout) = pending.pop() {
        paint_scrollbar(
            layout,
            viewport,
            scale_factor,
            clip_skip,
            context_world,
            canvas,
        );
        pending.extend(
            layout.children().iter().rev().filter(|child| {
                !child.establishes_stacking_context() && !child.is_positioned_auto()
            }),
        );
    }
}

fn paint_scrollbar(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    let track_color = Color::from_rgba8(15, 23, 42, 36);
    let track_style = DecorationStyle {
        corner_radius: CornerRadius::all(4.0 * scale_factor),
        ..DecorationStyle::default()
    };
    let thumb_color = Color::from_rgba8(71, 85, 105, 210);
    let thumb_style = DecorationStyle {
        corner_radius: CornerRadius::all(4.0 * scale_factor),
        ..DecorationStyle::default()
    };

    let Some(scroll) = layout.scroll else {
        return;
    };
    let clips = layout
        .clips
        .iter()
        .skip(clip_skip)
        .copied()
        .chain([scroll.clip])
        .map(|clip| localize_clip(clip, context_world))
        .collect::<Vec<_>>();
    for scrollbar in [scroll.horizontal, scroll.vertical].into_iter().flatten() {
        if !intersects_visible_area(scrollbar.track, &clips, viewport) {
            continue;
        }
        canvas.draw_decoration_with_clips(
            PaintLayer::Scrollbar,
            scaled_rect(scrollbar.track, scale_factor),
            BoxDecoration::Background {
                color: track_color,
                image: None,
            },
            track_style,
            clips
                .iter()
                .copied()
                .map(|clip| scaled_clip(clip, scale_factor)),
        );
        canvas.draw_decoration_with_clips(
            PaintLayer::Scrollbar,
            scaled_rect(scrollbar.thumb, scale_factor),
            BoxDecoration::Background {
                color: thumb_color,
                image: None,
            },
            thumb_style,
            clips
                .iter()
                .copied()
                .map(|clip| scaled_clip(clip, scale_factor)),
        );
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

    fn ordered_commands(canvas: &Canvas) -> Vec<&DrawCommand> {
        fn append<'a>(canvas: &'a Canvas, commands: &mut Vec<&'a DrawCommand>) {
            for layer in PaintLayer::ALL {
                for command in canvas.commands() {
                    let command_layer = match command {
                        DrawCommand::Decoration { layer, .. }
                        | DrawCommand::Group { layer, .. } => *layer,
                        DrawCommand::Box { .. } => PaintLayer::Block,
                        DrawCommand::Text { .. } => PaintLayer::Content,
                        DrawCommand::OverlayBox { .. } => PaintLayer::Overlay,
                    };
                    if command_layer != layer {
                        continue;
                    }
                    match command {
                        DrawCommand::Group { canvas, .. } => append(canvas, commands),
                        _ => commands.push(command),
                    }
                }
            }
        }

        let mut commands = Vec::new();
        append(canvas, &mut commands);
        commands
    }

    fn background_colors(canvas: &Canvas) -> Vec<Color> {
        ordered_commands(canvas)
            .into_iter()
            .filter_map(|command| match command {
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } => Some(*color),
                DrawCommand::Box { style, .. } => Some(style.background),
                _ => None,
            })
            .collect()
    }

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
        let background = canvas
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { image, .. },
                    style,
                    ..
                } => Some((image, style)),
                _ => None,
            })
            .expect("grouped background decoration");
        assert_eq!(background.1.opacity, 1.0);
        assert_eq!(background.1.transform, Transform::IDENTITY);
        assert!(matches!(
            background.0,
            Some(BackgroundImage::LinearGradient { .. })
        ));
        let shadow = canvas
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Shadow(shadow),
                    ..
                } => Some(shadow),
                _ => None,
            })
            .expect("grouped shadow decoration");
        assert_eq!(shadow.offset, [2.0, 4.0]);
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
        let image = ordered_commands(&canvas)
            .into_iter()
            .find_map(|command| match command {
                DrawCommand::Decoration {
                    decoration:
                        BoxDecoration::Background {
                            image: Some(BackgroundImage::Raster(image)),
                            ..
                        },
                    ..
                } => Some(image),
                _ => None,
            })
            .expect("expected raster background decoration");
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
            .any(|command| matches!(command, DrawCommand::Decoration { .. })));
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
                DrawCommand::Decoration { clips, .. } if !clips.is_empty() => Some(clips[0]),
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
    fn decorates_only_the_styled_nested_text() {
        let mut document = Document::new();
        let line = document.create_node(ElementKind::TextElement);
        let before = document.create_node(ElementKind::Text("before ".into()));
        let decorated = document.create_node(ElementKind::TextElement);
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
            panic!("nested text should produce one rich text layout");
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
        let (rect, border, style) = ordered_commands(&canvas)
            .into_iter()
            .find_map(|command| match command {
                DrawCommand::Decoration {
                    rect,
                    decoration: BoxDecoration::Border(border),
                    style,
                    ..
                } => Some((rect, border, style)),
                _ => None,
            })
            .expect("card should produce a border decoration");

        assert_eq!((rect.width, rect.height), (208.0, 108.0));
        assert_eq!(border.width, 4.0);
        assert_eq!(style.corner_radius.top_left, 8.0);
    }

    #[test]
    fn canvas_commands_follow_stacking_context_order() {
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
        for element in [high_descendant, middle, negative] {
            document
                .set_style(element, "position", Some("relative"))
                .unwrap();
        }

        document.insert(BODY_ID, ordinary, None).unwrap();
        document.insert(ordinary, high_descendant, None).unwrap();
        document.insert(BODY_ID, middle, None).unwrap();
        document.insert(BODY_ID, negative, None).unwrap();

        let canvas = build_canvas(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
        let backgrounds = background_colors(&canvas);

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
    fn block_decorations_paint_before_inline_text_and_outlines_paint_last() {
        let mut document = Document::new();
        let first = document.create_node(ElementKind::Div);
        let text = document.create_node(ElementKind::Text("front".into()));
        let second = document.create_node(ElementKind::Div);
        let positive = document.create_node(ElementKind::Div);

        document
            .set_style(first, "background-color", Some("red"))
            .unwrap();
        document
            .set_style(first, "outline-color", Some("black"))
            .unwrap();
        document
            .set_style(first, "outline-width", Some("2px"))
            .unwrap();
        document
            .set_style(second, "background-color", Some("blue"))
            .unwrap();
        document
            .set_style(positive, "background-color", Some("green"))
            .unwrap();
        document
            .set_style(positive, "position", Some("relative"))
            .unwrap();
        document.set_style(positive, "z-index", Some("1")).unwrap();
        for element in [first, second, positive] {
            document.set_style(element, "width", Some("40px")).unwrap();
            document.set_style(element, "height", Some("20px")).unwrap();
        }
        document.insert(BODY_ID, first, None).unwrap();
        document.insert(first, text, None).unwrap();
        document.insert(BODY_ID, second, None).unwrap();
        document.insert(BODY_ID, positive, None).unwrap();

        let canvas = build_canvas(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
        let commands = ordered_commands(&canvas);
        let red = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(255, 0, 0, 255)
                )
            })
            .unwrap();
        let blue = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(0, 0, 255, 255)
                )
            })
            .unwrap();
        let text = commands
            .iter()
            .position(|command| matches!(command, DrawCommand::Text { .. }))
            .unwrap();
        let green = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(0, 128, 0, 255)
                )
            })
            .unwrap();
        let outline = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Outline(_),
                        ..
                    }
                )
            })
            .unwrap();

        assert!(red < text);
        assert!(blue < text);
        assert!(text < green);
        assert!(green < outline);
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
            let clips = ordered_commands(&frame.canvas)
                .into_iter()
                .find_map(|command| match command {
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        clips,
                        ..
                    } if *color == Color::from_rgba8(0xff, 0, 0, 0xff) => Some(clips),
                    _ => None,
                })
                .expect("overflowing child background");
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
                    render::DrawCommand::Decoration {
                        layer: PaintLayer::Scrollbar,
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(15, 23, 42, 36)
                        || *color == Color::from_rgba8(71, 85, 105, 210)
                )
            })
            .count();
        assert_eq!(scrollbar_boxes, 4);
    }

    #[test]
    fn root_scrollbar_paints_above_absolute_and_below_fixed_content() {
        let mut document = Document::new();
        let content = document.create_node(ElementKind::Div);
        let absolute = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        document
            .set_style(BODY_ID, "overflow", Some("auto"))
            .unwrap();
        document.set_style(content, "width", Some("200px")).unwrap();
        document
            .set_style(content, "height", Some("200px"))
            .unwrap();
        document
            .set_style(absolute, "position", Some("absolute"))
            .unwrap();
        document.set_style(absolute, "right", Some("0px")).unwrap();
        document.set_style(absolute, "bottom", Some("0px")).unwrap();
        document.set_style(absolute, "width", Some("40px")).unwrap();
        document
            .set_style(absolute, "height", Some("40px"))
            .unwrap();
        document
            .set_style(absolute, "background-color", Some("#0000ff"))
            .unwrap();
        document
            .set_style(fixed, "position", Some("fixed"))
            .unwrap();
        document.set_style(fixed, "right", Some("0px")).unwrap();
        document.set_style(fixed, "bottom", Some("0px")).unwrap();
        document.set_style(fixed, "width", Some("40px")).unwrap();
        document.set_style(fixed, "height", Some("40px")).unwrap();
        document
            .set_style(fixed, "background-color", Some("#ff0000"))
            .unwrap();
        document.insert(BODY_ID, content, None).unwrap();
        document.insert(BODY_ID, absolute, None).unwrap();
        document.insert(BODY_ID, fixed, None).unwrap();

        let frame = build_frame(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
        assert!(
            frame
                .layout
                .scroll
                .is_some_and(|scroll| scroll.vertical.is_some()),
            "the root must have a vertical scrollbar for this ordering test"
        );

        let commands = ordered_commands(&frame.canvas);
        let absolute_index = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(0, 0, 255, 255)
                )
            })
            .expect("absolute background paint command");
        let scrollbar_index = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        layer: PaintLayer::Scrollbar,
                        ..
                    }
                )
            })
            .expect("scrollbar paint command");
        let fixed_index = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(255, 0, 0, 255)
                )
            })
            .expect("fixed background paint command");

        assert!(
            absolute_index < scrollbar_index && scrollbar_index < fixed_index,
            "root paint order must be absolute content, scrollbar, then viewport-fixed content"
        );
    }

    #[test]
    fn positioned_scroller_paints_its_scrollbar_inside_its_atomic_context() {
        let mut document = Document::new();
        let fixed = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        document
            .set_style(fixed, "position", Some("fixed"))
            .unwrap();
        document.set_style(fixed, "width", Some("100px")).unwrap();
        document.set_style(fixed, "height", Some("60px")).unwrap();
        document.set_style(fixed, "overflow", Some("auto")).unwrap();
        document
            .set_style(fixed, "background-color", Some("#ff0000"))
            .unwrap();
        document.set_style(content, "width", Some("100px")).unwrap();
        document
            .set_style(content, "height", Some("200px"))
            .unwrap();
        document.insert(BODY_ID, fixed, None).unwrap();
        document.insert(fixed, content, None).unwrap();

        let frame = build_frame(&document, 300.0, 200.0, 1.0, &mut TextSystem::new());
        assert!(
            !frame.canvas.commands().iter().any(|command| matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::Scrollbar,
                    ..
                }
            )),
            "an atomic scroller must not leak its scrollbar into the parent canvas"
        );

        let group = frame
            .canvas
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Group { canvas, .. }
                    if canvas.commands().iter().any(|command| {
                        matches!(
                            command,
                            DrawCommand::Decoration {
                                layer: PaintLayer::Scrollbar,
                                ..
                            }
                        )
                    }) =>
                {
                    Some(canvas)
                }
                _ => None,
            })
            .expect("positioned scroller group containing its scrollbar");
        let commands = ordered_commands(group);
        let background_index = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        decoration: BoxDecoration::Background { color, .. },
                        ..
                    } if *color == Color::from_rgba8(255, 0, 0, 255)
                )
            })
            .expect("positioned scroller background");
        let scrollbar_index = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    DrawCommand::Decoration {
                        layer: PaintLayer::Scrollbar,
                        ..
                    }
                )
            })
            .expect("positioned scroller scrollbar");

        assert!(background_index < scrollbar_index);
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
        let backgrounds = background_colors(&frame.canvas);

        assert!(!backgrounds.contains(&Color::from_rgba8(0xff, 0, 0, 0xff)));
        assert!(!backgrounds.contains(&Color::from_rgba8(0, 0xff, 0, 0xff)));
        assert!(backgrounds.contains(&Color::from_rgba8(0, 0, 0xff, 0xff)));
    }
}
