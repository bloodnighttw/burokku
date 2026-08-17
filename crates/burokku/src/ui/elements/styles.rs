
pub mod common;
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

/// A strongly typed color stored in authoritative element data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl RgbaColor {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: u8::MAX,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let hex = value.strip_prefix('#')?;
        if !hex.as_bytes().iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        match hex.len() {
            3 => Some(Self::rgb(
                expand_hex(hex.as_bytes()[0])?,
                expand_hex(hex.as_bytes()[1])?,
                expand_hex(hex.as_bytes()[2])?,
            )),
            6 | 8 => Some(Self {
                red: parse_hex_byte(&hex[0..2])?,
                green: parse_hex_byte(&hex[2..4])?,
                blue: parse_hex_byte(&hex[4..6])?,
                alpha: if hex.len() == 8 {
                    parse_hex_byte(&hex[6..8])?
                } else {
                    u8::MAX
                },
            }),
            _ => None,
        }
    }
}

fn expand_hex(value: u8) -> Option<u8> {
    let digit = (value as char).to_digit(16)? as u8;
    Some((digit << 4) | digit)
}

fn parse_hex_byte(value: &str) -> Option<u8> {
    u8::from_str_radix(value, 16).ok()
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

fn to_taffy_auto(value: LengthPercentage) -> taffy::LengthPercentageAuto {
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
