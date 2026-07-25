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

    pub fn intersection(self, other: Self) -> Self {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        Self::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && x >= self.x
            && x < self.x + self.width
            && y >= self.y
            && y < self.y + self.height
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

/// A rectangular clip edge with optional rounded corners.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clip {
    pub rect: Rect,
    pub corner_radius: CornerRadius,
}

impl Clip {
    pub const fn new(rect: Rect, corner_radius: CornerRadius) -> Self {
        Self {
            rect,
            corner_radius,
        }
    }

    pub const fn rectangular(rect: Rect) -> Self {
        Self::new(rect, CornerRadius::ZERO)
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }

        let half_size = [self.rect.width * 0.5, self.rect.height * 0.5];
        let position = [
            x - (self.rect.x + half_size[0]),
            y - (self.rect.y + half_size[1]),
        ];
        let radii = self.corner_radius.normalized(self.rect);
        let radius = if position[1] < 0.0 {
            if position[0] < 0.0 {
                radii[0]
            } else {
                radii[1]
            }
        } else if position[0] < 0.0 {
            radii[3]
        } else {
            radii[2]
        };
        let q = [
            position[0].abs() - half_size[0] + radius,
            position[1].abs() - half_size[1] + radius,
        ];
        q[0].max(q[1]).min(0.0) + q[0].max(0.0).hypot(q[1].max(0.0)) <= 0.0
    }
}
