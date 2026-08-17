/// A thread-safe length that is either absolute logical pixels or a fraction
/// of the containing size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LengthPercentage {
    Length(f32),
    Percent(f32),
}

impl LengthPercentage {
    pub const ZERO: Self = Self::Length(0.0);

    pub const fn length(value: f32) -> Self {
        Self::Length(value)
    }

    /// `value` uses the `0.0..=1.0` representation expected by Taffy.
    pub const fn percent(value: f32) -> Self {
        Self::Percent(value)
    }

    pub fn to_taffy(self) -> taffy::LengthPercentage {
        match self {
            Self::Length(value) => taffy::LengthPercentage::length(value),
            Self::Percent(value) => taffy::LengthPercentage::percent(value),
        }
    }
}

impl Default for LengthPercentage {
    fn default() -> Self {
        Self::ZERO
    }
}


/// A thread-safe preferred size used by flex items.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Dimension {
    Length(f32),
    Percent(f32),
    #[default]
    Auto,
}

impl Dimension {
    pub const fn length(value: f32) -> Self {
        Self::Length(value)
    }

    pub const fn percent(value: f32) -> Self {
        Self::Percent(value)
    }

    pub const fn auto() -> Self {
        Self::Auto
    }

    pub fn to_taffy(self) -> taffy::Dimension {
        match self {
            Self::Length(value) => taffy::Dimension::length(value),
            Self::Percent(value) => taffy::Dimension::percent(value),
            Self::Auto => taffy::Dimension::auto(),
        }
    }
}


pub(crate) fn parse_length_percentage(value: &str) -> Option<LengthPercentage> {
    let value = value.trim();
    if let Some(value) = value.strip_suffix("px") {
        return value.trim().parse().ok().map(LengthPercentage::Length);
    }
    if let Some(value) = value.strip_suffix('%') {
        return value
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| LengthPercentage::Percent(value / 100.0));
    }
    value.parse().ok().map(LengthPercentage::Length)
}

pub(crate) fn parse_dimension(value: &str) -> Option<Dimension> {
    if value.trim() == "auto" {
        Some(Dimension::Auto)
    } else {
        parse_length_percentage(value).map(|value| match value {
            LengthPercentage::Length(value) => Dimension::Length(value),
            LengthPercentage::Percent(value) => Dimension::Percent(value),
        })
    }
}

pub fn to_taffy_auto(value: LengthPercentage) -> taffy::LengthPercentageAuto {
    match value {
        LengthPercentage::Length(value) => taffy::LengthPercentageAuto::length(value),
        LengthPercentage::Percent(value) => taffy::LengthPercentageAuto::percent(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn style_primitives_are_send_and_sync() {
        assert_send_sync::<LengthPercentage>();
        assert_send_sync::<Dimension>();
    }

    #[test]
    fn primitives_convert_to_taffy_on_mts() {
        assert_eq!(
            LengthPercentage::length(12.0).to_taffy(),
            taffy::LengthPercentage::length(12.0)
        );
        assert_eq!(
            LengthPercentage::percent(0.5).to_taffy(),
            taffy::LengthPercentage::percent(0.5)
        );
        assert_eq!(Dimension::auto().to_taffy(), taffy::Dimension::auto());
    }
}