/// A rectangle in canvas pixels. The origin is at the target's top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Per-corner radii, in top-left, top-right, bottom-right, bottom-left order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadius {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    pub(crate) fn normalized(self, rect: Rect) -> [f32; 4] {
        let max = rect.width.min(rect.height).max(0.0) * 0.5;
        [
            self.top_left.clamp(0.0, max),
            self.top_right.clamp(0.0, max),
            self.bottom_right.clamp(0.0, max),
            self.bottom_left.clamp(0.0, max),
        ]
    }
}
