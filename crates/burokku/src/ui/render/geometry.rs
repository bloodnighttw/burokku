use render::{Clip, Rect};

use crate::ui::layouts::{Layout, LayoutKind};

pub(super) fn visual_bounds(layout: &Layout) -> Rect {
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

pub(super) fn intersects_visible_area(mut bounds: Rect, clips: &[Clip], viewport: Rect) -> bool {
    bounds = bounds.intersection(viewport);
    for clip in clips {
        bounds = bounds.intersection(clip.bounds());
    }
    bounds.width > 0.0 && bounds.height > 0.0
}
