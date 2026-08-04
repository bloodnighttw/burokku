use taffy::prelude::{Dimension, TaffyAuto};

/// A CSS maximum size that accepts `<length-percentage> | none`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub(crate) enum MaxSizeValue {
    #[default]
    None,
    Px(f32),
    Percent(f32),
}

impl From<MaxSizeValue> for Dimension {
    fn from(value: MaxSizeValue) -> Self {
        match value {
            MaxSizeValue::None => Self::AUTO,
            MaxSizeValue::Px(value) => Self::length(value),
            MaxSizeValue::Percent(value) => Self::percent(value / 100.0),
        }
    }
}
