#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    // to determine if the rectangle has a non-zero area
    pub fn has_area(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}
