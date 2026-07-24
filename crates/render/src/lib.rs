//! A small 2D drawing library backed by WebGPU.
//!
//! The application owns its window and surface. This crate owns drawing
//! pipelines and renders a [`Canvas`] into the surface it is given.

mod drawing;
mod renderer;
mod text_system;

pub use drawing::{
    Border, BoxStyle, Canvas, Clip, Color, CornerRadius, DrawCommand, FontFamily, FontStyle,
    Outline, Rect, TextAlign, TextDecorationLine, TextOverflowWrap, TextStyle, TextWhiteSpace,
    TextWordBreak, TextWrap,
};
pub use renderer::{RenderError, RenderTimings, Renderer, SurfaceSize};
pub use text_system::{
    TextConstraints, TextLayoutMetrics, TextMetrics, TextRunMetrics, TextSystem, TextWidth,
};
pub use wgpu;
