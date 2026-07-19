use super::{Color, CornerRadius};

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
    pub corner_radius: CornerRadius,
    pub border: Option<Border>,
    pub outline: Option<Outline>,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            background: Color::TRANSPARENT,
            corner_radius: CornerRadius::ZERO,
            border: None,
            outline: None,
        }
    }
}
