use render::{BoxStyle, Canvas, PaintLayer, Rect, Transform};

use super::{
    decorations::{paint_box_decorations, paint_text_decorations},
    geometry::{intersects_visible_area, visual_bounds},
    scale::{scaled_box_style, scaled_clip, scaled_text_spans, scaled_text_style},
    transform::localize_clip,
};
use crate::ui::layouts::{Layout, LayoutKind};

#[expect(
    clippy::too_many_arguments,
    reason = "painting one retained layout needs its phase and render context"
)]
pub(super) fn paint_phased_layout(
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
            let mut text_group = Canvas::new();
            paint_text_decorations(
                bounds,
                &scaled_spans,
                runs,
                scale_factor,
                false,
                clips.iter().copied(),
                &mut text_group,
            );
            text_group.draw_rich_text_with_clips(
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
                true,
                clips.iter().copied(),
                &mut text_group,
            );
            canvas.draw_group_in_layer(
                PaintLayer::Content,
                text_group,
                [
                    (layout.x + layout.width * 0.5) * scale_factor,
                    (layout.y + layout.height * 0.5) * scale_factor,
                ],
                Transform::IDENTITY,
                1.0,
                [],
            );
        }
    }
}
