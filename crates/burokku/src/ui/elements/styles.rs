use taffy::{
    geometry::{Rect, Size},
    AlignSelf,
};

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

/// Layout and paint properties shared by block, flex, and grid elements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommonStyle {
    pub size: Size<Dimension>,
    pub padding: Rect<LengthPercentage>,
    pub margin: Rect<LengthPercentage>,
    pub background_color: Option<RgbaColor>,
    pub flex_basis: Dimension,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub align_self: Option<AlignSelf>,
}

impl CommonStyle {
    pub(crate) fn supports(name: &str) -> bool {
        matches!(
            name,
            "width"
                | "height"
                | "padding"
                | "margin"
                | "background-color"
                | "flex-basis"
                | "flex-grow"
                | "flex-shrink"
                | "align-self"
        )
    }

    pub(crate) fn set_property(&mut self, name: &str, value: &str) -> bool {
        match name {
            "width" => parse_dimension(value).is_some_and(|value| {
                self.size.width = value;
                true
            }),
            "height" => parse_dimension(value).is_some_and(|value| {
                self.size.height = value;
                true
            }),
            "padding" => parse_length_percentage(value).is_some_and(|value| {
                self.padding = Rect {
                    left: value,
                    right: value,
                    top: value,
                    bottom: value,
                };
                true
            }),
            "margin" => parse_length_percentage(value).is_some_and(|value| {
                self.margin = Rect {
                    left: value,
                    right: value,
                    top: value,
                    bottom: value,
                };
                true
            }),
            "background-color" => RgbaColor::parse(value).is_some_and(|value| {
                self.background_color = Some(value);
                true
            }),
            "flex-basis" => parse_dimension(value).is_some_and(|value| {
                self.flex_basis = value;
                true
            }),
            "flex-grow" => value.parse().ok().is_some_and(|value| {
                self.flex_grow = value;
                true
            }),
            "flex-shrink" => value.parse().ok().is_some_and(|value| {
                self.flex_shrink = value;
                true
            }),
            "align-self" => value.parse().ok().is_some_and(|value| {
                self.align_self = Some(value);
                true
            }),
            _ => false,
        }
    }

    pub(crate) fn remove_property(&mut self, name: &str) -> bool {
        if !Self::supports(name) {
            return false;
        }
        let defaults = Self::default();
        match name {
            "width" => self.size.width = defaults.size.width,
            "height" => self.size.height = defaults.size.height,
            "padding" => self.padding = defaults.padding,
            "margin" => self.margin = defaults.margin,
            "background-color" => self.background_color = defaults.background_color,
            "flex-basis" => self.flex_basis = defaults.flex_basis,
            "flex-grow" => self.flex_grow = defaults.flex_grow,
            "flex-shrink" => self.flex_shrink = defaults.flex_shrink,
            "align-self" => self.align_self = defaults.align_self,
            _ => unreachable!("support was checked above"),
        }
        true
    }

    pub(crate) fn apply_to_taffy(self, style: &mut taffy::Style<String>) {
        style.size = Size {
            width: self.size.width.to_taffy(),
            height: self.size.height.to_taffy(),
        };
        style.padding = Rect {
            left: self.padding.left.to_taffy(),
            right: self.padding.right.to_taffy(),
            top: self.padding.top.to_taffy(),
            bottom: self.padding.bottom.to_taffy(),
        };
        style.margin = Rect {
            left: to_taffy_auto(self.margin.left),
            right: to_taffy_auto(self.margin.right),
            top: to_taffy_auto(self.margin.top),
            bottom: to_taffy_auto(self.margin.bottom),
        };
        style.flex_basis = self.flex_basis.to_taffy();
        style.flex_grow = self.flex_grow;
        style.flex_shrink = self.flex_shrink;
        style.align_self = self.align_self;
    }
}

impl Default for CommonStyle {
    fn default() -> Self {
        Self {
            size: Size {
                width: Dimension::Auto,
                height: Dimension::Auto,
            },
            padding: Rect {
                left: LengthPercentage::ZERO,
                right: LengthPercentage::ZERO,
                top: LengthPercentage::ZERO,
                bottom: LengthPercentage::ZERO,
            },
            margin: Rect {
                left: LengthPercentage::ZERO,
                right: LengthPercentage::ZERO,
                top: LengthPercentage::ZERO,
                bottom: LengthPercentage::ZERO,
            },
            background_color: None,
            flex_basis: Dimension::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            align_self: None,
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
