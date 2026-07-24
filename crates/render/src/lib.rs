//! A small 2D drawing library backed by WebGPU.
//!
//! The application owns its window and surface. This crate owns drawing
//! pipelines and renders a [`Canvas`] into the surface it is given.

mod drawing;
mod renderer;
mod text_system;

pub use drawing::{
    Border, BorderSide, BorderStyle, BoxStyle, Canvas, Clip, Color, CornerRadius, CornerSize,
    DrawCommand, FontFamily, FontStyle, Outline, Rect, TextAlign, TextDecorationLine, TextStyle,
    TextWhiteSpace, TextWrap,
};
pub use renderer::{RenderError, RenderTimings, Renderer, SurfaceSize};
pub use text_system::{TextConstraints, TextMetrics, TextSystem, TextWidth};
pub use wgpu;
