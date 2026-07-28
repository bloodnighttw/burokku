use super::{Color, CornerRadius};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// CSS-compatible affine matrix `[a, b, c, d, e, f]`.
    pub matrix: [f32; 6],
}

impl Transform {
    pub const IDENTITY: Self = Self {
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset: [f32; 2],
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Tightly packed, top-to-bottom RGBA8 pixels.
    pub pixels: Arc<[u8]>,
}

impl RasterImage {
    pub fn new(width: u32, height: u32, pixels: impl Into<Arc<[u8]>>) -> Option<Self> {
        let pixels = pixels.into();
        let expected = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?;
        (width > 0 && height > 0 && pixels.len() == expected).then_some(Self {
            width,
            height,
            pixels,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Normalized position along the gradient axis.
    pub position: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundImage {
    LinearGradient {
        direction: [f32; 2],
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        stops: Vec<GradientStop>,
    },
    Raster(RasterImage),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    /// Width drawn inside the box edge.
    pub width: f32,
    pub color: Color,
}

impl Border {
    pub const fn new(width: f32, color: Color) -> Self {
        Self { width, color }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    /// Width drawn outside the box edge.
    pub width: f32,
    /// Transparent distance between the box edge and outline.
    pub offset: f32,
    pub color: Color,
}

impl Outline {
    pub const fn new(width: f32, offset: f32, color: Color) -> Self {
        Self {
            width,
            offset,
            color,
        }
    }
}

/// Complete compatibility style for drawing a box in one command.
///
/// Layout engines that need CSS-like phase ordering should split this into
/// [`BoxDecoration`] commands with a shared [`DecorationStyle`].
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle {
    pub background: Color,
    pub background_image: Option<BackgroundImage>,
    pub corner_radius: CornerRadius,
    pub border: Option<Border>,
    pub outline: Option<Outline>,
    pub opacity: f32,
    pub transform: Transform,
    pub shadows: Vec<BoxShadow>,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            background: Color::TRANSPARENT,
            background_image: None,
            corner_radius: CornerRadius::ZERO,
            border: None,
            outline: None,
            opacity: 1.0,
            transform: Transform::IDENTITY,
            shadows: Vec::new(),
        }
    }
}

/// Geometry and effects shared by one independently ordered box decoration.
///
/// A decoration intentionally contains only one visual operation. This lets
/// callers place backgrounds, borders, shadows, and outlines into different
/// paint layers without rebuilding a monolithic [`BoxStyle`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorationStyle {
    pub corner_radius: CornerRadius,
    pub opacity: f32,
    pub transform: Transform,
}

impl Default for DecorationStyle {
    fn default() -> Self {
        Self {
            corner_radius: CornerRadius::ZERO,
            opacity: 1.0,
            transform: Transform::IDENTITY,
        }
    }
}

/// One independently drawable part of a CSS-like box.
#[derive(Clone, Debug, PartialEq)]
pub enum BoxDecoration {
    /// Background color and optional image, clipped to the rounded box.
    Background {
        color: Color,
        image: Option<BackgroundImage>,
    },
    /// Border drawn inside the box edge.
    Border(Border),
    /// Outline drawn outside the box edge.
    Outline(Outline),
    /// One outer or inset shadow.
    Shadow(BoxShadow),
}
