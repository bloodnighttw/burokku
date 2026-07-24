use super::{Color, CornerRadius};

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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundImage {
    LinearGradient {
        direction: [f32; 2],
        start: Color,
        end: Color,
    },
    RadialGradient {
        start: Color,
        end: Color,
    },
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxStyle {
    pub background: Color,
    pub background_image: Option<BackgroundImage>,
    pub corner_radius: CornerRadius,
    pub border: Option<Border>,
    pub outline: Option<Outline>,
    pub opacity: f32,
    pub transform: Transform,
    pub shadow: Option<BoxShadow>,
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
            shadow: None,
        }
    }
}
