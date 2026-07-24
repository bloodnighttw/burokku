use super::{Color, CornerRadius};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    /// Widths drawn inside the box edge.
    pub top_width: f32,
    pub right_width: f32,
    pub bottom_width: f32,
    pub left_width: f32,
    pub color: Color,
}

impl Border {
    pub const fn new(width: f32, color: Color) -> Self {
        Self::per_side(width, width, width, width, color)
    }

    pub const fn per_side(
        top_width: f32,
        right_width: f32,
        bottom_width: f32,
        left_width: f32,
        color: Color,
    ) -> Self {
        Self {
            top_width,
            right_width,
            bottom_width,
            left_width,
            color,
        }
    }

    pub const fn widths(self) -> [f32; 4] {
        [
            self.top_width,
            self.right_width,
            self.bottom_width,
            self.left_width,
        ]
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
