use super::measurement::{Auto, Percent, Px};

/// The supported values of CSS `position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

/// The supported CSS border line styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum BorderStyle {
    None,
    Hidden,
    Dotted,
    Dashed,
    #[default]
    Solid,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

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

/// The horizontal and vertical radius of one box corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerRadiusValue {
    pub(crate) horizontal: LengthPercentageValue,
    pub(crate) vertical: LengthPercentageValue,
}

impl CornerRadiusValue {
    pub(crate) const ZERO: Self = Self::all(LengthPercentageValue::ZERO);

    pub(crate) const fn all(value: LengthPercentageValue) -> Self {
        Self {
            horizontal: value,
            vertical: value,
        }
    }

    pub(crate) const fn new(
        horizontal: LengthPercentageValue,
        vertical: LengthPercentageValue,
    ) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }
}

impl Default for CornerRadiusValue {
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
