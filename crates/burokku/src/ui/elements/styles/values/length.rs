use taffy::prelude::LengthPercentage;

/// A CSS value that accepts an absolute length only.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) enum LengthValue {
    Px(f32),
}

impl LengthValue {
    pub(crate) const ZERO: Self = Self::Px(0.0);
}

impl Default for LengthValue {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<LengthValue> for f32 {
    fn from(value: LengthValue) -> Self {
        match value {
            LengthValue::Px(value) => value,
        }
    }
}

impl From<LengthValue> for LengthPercentage {
    fn from(value: LengthValue) -> Self {
        Self::length(f32::from(value))
    }
}
