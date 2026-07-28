use taffy::prelude::LengthPercentage;

use super::MaxSizeValue;

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

impl From<LengthPercentageValue> for LengthPercentage {
    fn from(value: LengthPercentageValue) -> Self {
        match value {
            LengthPercentageValue::Px(value) => Self::length(value),
            LengthPercentageValue::Percent(value) => Self::percent(value / 100.0),
        }
    }
}

impl From<LengthPercentageValue> for MaxSizeValue {
    fn from(value: LengthPercentageValue) -> Self {
        match value {
            LengthPercentageValue::Px(value) => Self::Px(value),
            LengthPercentageValue::Percent(value) => Self::Percent(value),
        }
    }
}
