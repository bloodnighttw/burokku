use super::{Color, CornerRadius};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum BorderStyle {
    None = 0,
    Hidden = 1,
    Dotted = 2,
    Dashed = 3,
    #[default]
    Solid = 4,
    Double = 5,
    Groove = 6,
    Ridge = 7,
    Inset = 8,
    Outset = 9,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSide {
    pub width: f32,
    pub color: Color,
    pub style: BorderStyle,
}

impl BorderSide {
    pub const fn new(width: f32, color: Color, style: BorderStyle) -> Self {
        Self {
            width,
            color,
            style,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
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
        Self::sides(
            BorderSide::new(top_width, color, BorderStyle::Solid),
            BorderSide::new(right_width, color, BorderStyle::Solid),
            BorderSide::new(bottom_width, color, BorderStyle::Solid),
            BorderSide::new(left_width, color, BorderStyle::Solid),
        )
    }

    pub const fn sides(
        top: BorderSide,
        right: BorderSide,
        bottom: BorderSide,
        left: BorderSide,
    ) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn widths(self) -> [f32; 4] {
        [
            self.top.width,
            self.right.width,
            self.bottom.width,
            self.left.width,
        ]
    }

    pub const fn colors(self) -> [Color; 4] {
        [
            self.top.color,
            self.right.color,
            self.bottom.color,
            self.left.color,
        ]
    }

    pub const fn styles(self) -> [BorderStyle; 4] {
        [
            self.top.style,
            self.right.style,
            self.bottom.style,
            self.left.style,
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
