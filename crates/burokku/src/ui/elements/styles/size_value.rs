use crate::ui::elements::styles::measurement::{Auto, Percent, Px};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub enum SizeValue {
    #[default]
    Auto,
    Px(i32),
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
