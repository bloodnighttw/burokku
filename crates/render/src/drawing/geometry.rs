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

/// The horizontal and vertical radius of one corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerSize {
    pub x: f32,
    pub y: f32,
}

impl CornerSize {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(radius: f32) -> Self {
        Self {
            x: radius,
            y: radius,
        }
    }

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Per-corner elliptical radii.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadius {
    pub top_left: CornerSize,
    pub top_right: CornerSize,
    pub bottom_right: CornerSize,
    pub bottom_left: CornerSize,
}

impl CornerRadius {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: CornerSize::all(radius),
            top_right: CornerSize::all(radius),
            bottom_right: CornerSize::all(radius),
            bottom_left: CornerSize::all(radius),
        }
    }

    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self::elliptical(
            CornerSize::all(top_left),
            CornerSize::all(top_right),
            CornerSize::all(bottom_right),
            CornerSize::all(bottom_left),
        )
    }

    pub const fn elliptical(
        top_left: CornerSize,
        top_right: CornerSize,
        bottom_right: CornerSize,
        bottom_left: CornerSize,
    ) -> Self {
        Self {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    pub(crate) fn normalized(self, rect: Rect) -> ([f32; 4], [f32; 4]) {
        let mut x = [
            self.top_left.x.max(0.0),
            self.top_right.x.max(0.0),
            self.bottom_right.x.max(0.0),
            self.bottom_left.x.max(0.0),
        ];
        let mut y = [
            self.top_left.y.max(0.0),
            self.top_right.y.max(0.0),
            self.bottom_right.y.max(0.0),
            self.bottom_left.y.max(0.0),
        ];
        let width = rect.width.max(0.0);
        let height = rect.height.max(0.0);
        let ratio = |available: f32, requested: f32| {
            if requested > 0.0 {
                available / requested
            } else {
                1.0
            }
        };
        let scale = 1.0_f32
            .min(ratio(width, x[0] + x[1]))
            .min(ratio(width, x[3] + x[2]))
            .min(ratio(height, y[0] + y[3]))
            .min(ratio(height, y[1] + y[2]));
        for value in x.iter_mut().chain(y.iter_mut()) {
            *value *= scale;
        }
        (x, y)
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
        let (radii_x, radii_y) = self.corner_radius.normalized(self.rect);
        let radius = if position[1] < 0.0 {
            if position[0] < 0.0 {
                [radii_x[0], radii_y[0]]
            } else {
                [radii_x[1], radii_y[1]]
            }
        } else if position[0] < 0.0 {
            [radii_x[3], radii_y[3]]
        } else {
            [radii_x[2], radii_y[2]]
        };
        let q = [
            position[0].abs() - half_size[0] + radius[0],
            position[1].abs() - half_size[1] + radius[1],
        ];
        if q[0] <= 0.0 || q[1] <= 0.0 || radius[0] <= 0.0 || radius[1] <= 0.0 {
            true
        } else {
            (q[0] / radius[0]).powi(2) + (q[1] / radius[1]).powi(2) <= 1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elliptical_clip_uses_independent_horizontal_and_vertical_radii() {
        let clip = Clip::new(
            Rect::new(0.0, 0.0, 100.0, 50.0),
            CornerRadius::elliptical(
                CornerSize::new(40.0, 10.0),
                CornerSize::ZERO,
                CornerSize::ZERO,
                CornerSize::ZERO,
            ),
        );

        assert!(!clip.contains(2.0, 2.0));
        assert!(clip.contains(10.0, 5.0));
        assert!(clip.contains(50.0, 1.0));
    }

    #[test]
    fn normalizes_overlapping_radii_with_one_css_scale_factor() {
        let radii = CornerRadius::elliptical(
            CornerSize::new(80.0, 40.0),
            CornerSize::new(80.0, 20.0),
            CornerSize::ZERO,
            CornerSize::ZERO,
        );

        let (x, y) = radii.normalized(Rect::new(0.0, 0.0, 100.0, 100.0));

        assert_eq!(x, [50.0, 50.0, 0.0, 0.0]);
        assert_eq!(y, [25.0, 12.5, 0.0, 0.0]);
    }
}
