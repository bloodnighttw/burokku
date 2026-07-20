/// A size expressed in logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalSize<T = f64> {
    pub width: T,
    pub height: T,
}

impl<T> LogicalSize<T> {
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

/// A size expressed in physical pixels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PhysicalSize<T = u32> {
    pub width: T,
    pub height: T,
}

impl<T> PhysicalSize<T> {
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

/// A position expressed in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalPosition<T = f64> {
    pub x: T,
    pub y: T,
}

impl<T> PhysicalPosition<T> {
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}
