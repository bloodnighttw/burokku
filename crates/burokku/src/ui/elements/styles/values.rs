use super::measurement::{Auto, Percent, Px};

/// The supported forms of CSS overflow, preserving `auto` versus `scroll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Auto,
    Scroll,
}

/// The supported forms of CSS `z-index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ZIndex {
    #[default]
    Auto,
    Value(i32),
}

/// The supported values of CSS `isolation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Isolation {
    #[default]
    Auto,
    Isolate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Transform {
    pub(crate) matrix: [f32; 6],
}

impl Transform {
    pub(crate) const IDENTITY: Self = Self {
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shadow {
    pub(crate) offset_x: f32,
    pub(crate) offset_y: f32,
    pub(crate) blur: f32,
    pub(crate) spread: f32,
    pub(crate) color: [u8; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundImage {
    LinearGradient {
        direction: [f32; 2],
        start: [u8; 4],
        end: [u8; 4],
    },
    RadialGradient {
        start: [u8; 4],
        end: [u8; 4],
    },
    Raster(render::RasterImage),
}

/// A CSS size that accepts `<length-percentage> | auto`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) enum SizeValue {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}

impl SizeValue {
    pub(crate) const ZERO: Self = Self::Px(0.0);

    pub(crate) const fn px(value: f32) -> Self {
        Self::Px(value)
    }

    pub(crate) const fn percent(value: f32) -> Self {
        Self::Percent(value)
    }
}

/// A CSS maximum size that accepts `<length-percentage> | none`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) enum MaxSizeValue {
    #[default]
    None,
    Px(f32),
    Percent(f32),
}

/// A CSS value that accepts a length or percentage, but never `auto`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) enum LengthPercentageValue {
    Px(f32),
    Percent(f32),
}

impl LengthPercentageValue {
    pub(crate) const ZERO: Self = Self::Px(0.0);
}

impl Default for LengthPercentageValue {
    fn default() -> Self {
        Self::ZERO
    }
}

/// A CSS value that accepts an absolute length only.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) enum LengthValue {
    Px(f32),
}

impl LengthValue {
    pub(crate) const ZERO: Self = Self::Px(0.0);

    pub(crate) const fn px(self) -> f32 {
        match self {
            Self::Px(value) => value,
        }
    }
}

impl Default for LengthValue {
    fn default() -> Self {
        Self::ZERO
    }
}

/// The supported forms of CSS `line-height`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) enum LineHeightValue {
    Normal,
    Number(f32),
    Px(f32),
    Percent(f32),
}

impl From<Px> for SizeValue {
    fn from(value: Px) -> Self {
        Self::Px(value.value())
    }
}

impl From<Percent> for SizeValue {
    fn from(value: Percent) -> Self {
        Self::Percent(value.value())
    }
}

impl From<Auto> for SizeValue {
    fn from(_: Auto) -> Self {
        Self::Auto
    }
}

impl From<Px> for LengthPercentageValue {
    fn from(value: Px) -> Self {
        Self::Px(value.value())
    }
}

impl From<Percent> for LengthPercentageValue {
    fn from(value: Percent) -> Self {
        Self::Percent(value.value())
    }
}

impl From<Px> for LengthValue {
    fn from(value: Px) -> Self {
        Self::Px(value.value())
    }
}
