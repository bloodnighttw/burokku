mod decorations;
mod geometry;
mod node;
mod paint;
mod scale;
mod scrollbars;
mod transform;

use std::collections::HashMap;

use render::{Canvas, Color, Rect, TextSystem};

use super::{
    elements::Document,
    layouts::{compute_layout, compute_layout_with_scroll, Layout, ScrollOffset},
};
use paint::paint_layout;

#[cfg(test)]
use render::{BoxDecoration, Clip, CornerRadius, PaintLayer, Transform};
#[cfg(test)]
use scale::scaled_clip;

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

#[cfg(test)]
mod tests;
