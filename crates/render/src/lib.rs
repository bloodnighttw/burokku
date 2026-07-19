//! A small 2D drawing library backed by WebGPU.
//!
//! The application owns its window and surface. This crate owns drawing
//! pipelines and renders a [`Canvas`] into the surface it is given.

mod drawing;
mod renderer;

pub use drawing::{
    Border, BoxStyle, Canvas, Color, CornerRadius, DrawCommand, FontFamily, Outline, Rect,
    TextStyle, TextWrap,
};
pub use renderer::{RenderError, Renderer, SurfaceSize};
pub use wgpu;
