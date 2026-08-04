use render::{
    BoxDecoration, Canvas, Color, CornerRadius, DecorationStyle, PaintLayer, Rect, Transform,
};

use super::{
    geometry::intersects_visible_area,
    scale::{scaled_clip, scaled_rect},
    transform::localize_clip,
};
use crate::ui::layouts::{Layout, Stacking};

pub(super) fn paint_context_scrollbars(
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
        pending.extend(layout.children().iter().rev().filter(|child| {
            !child.establishes_stacking_context()
                && !child.is_positioned_auto()
                && !child.is_flex_or_grid_item_auto()
        }));
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
