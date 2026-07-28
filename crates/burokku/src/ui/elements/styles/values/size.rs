use taffy::prelude::{Dimension, LengthPercentageAuto, TaffyAuto};

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

impl From<SizeValue> for Dimension {
    fn from(value: SizeValue) -> Self {
        match value {
            SizeValue::Auto => Self::AUTO,
            SizeValue::Px(value) => Self::length(value),
            SizeValue::Percent(value) => Self::percent(value / 100.0),
        }
    }
}

impl From<SizeValue> for LengthPercentageAuto {
    fn from(value: SizeValue) -> Self {
        match value {
            SizeValue::Auto => Self::AUTO,
            SizeValue::Px(value) => Self::length(value),
            SizeValue::Percent(value) => Self::percent(value / 100.0),
        }
    }
}
