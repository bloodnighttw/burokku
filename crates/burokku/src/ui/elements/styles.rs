pub mod flex;
pub mod grid;

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
