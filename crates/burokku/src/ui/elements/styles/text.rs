//! Box and inherited typography styles for styled `<text>` elements.

use crate::ui::elements::{
    styles::{color::RgbaColor, common::CommonStyle},
    traits::Styles,
};

/// A validated CSS-like font weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontWeight(u16);

impl FontWeight {
    pub const NORMAL: Self = Self(400);
    pub const BOLD: Self = Self(700);

    /// CSS Fonts permits numeric weights from 1 through 1000.
    pub const fn new(value: u16) -> Option<Self> {
        if value >= 1 && value <= 1000 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// The computed line-height value passed to the future text layout engine.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineHeight {
    #[default]
    Normal,
    /// A multiplier of the computed font size.
    Factor(f32),
    /// An absolute logical-pixel height.
    Length(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrap {
    #[default]
    Wrap,
    NoWrap,
}

/// Typography declarations on a styled `<text>` element.
///
/// `None` means that the declaration inherits from the enclosing styled text
/// run. Keeping declarations optional lets the shaping layer distinguish an
/// omitted property from an explicit value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextStyle {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub font_weight: Option<FontWeight>,
    pub color: Option<RgbaColor>,
    pub line_height: Option<LineHeight>,
    pub wrap: Option<TextWrap>,
}

impl TextStyle {
    pub fn supports_property(property: &str) -> bool {
        matches!(
            property,
            "font-family" | "font-size" | "font-weight" | "color" | "line-height" | "text-wrap"
        )
    }

    /// Sets a supported declaration. Returns false for unsupported properties
    /// and invalid values.
    pub fn set_property(&mut self, property: &str, value: &str) -> bool {
        match property {
            "font-family" => parse_font_family(value).is_some_and(|value| {
                self.font_family = Some(value);
                true
            }),
            "font-size" => parse_non_negative_px(value).is_some_and(|value| {
                self.font_size = Some(value);
                true
            }),
            "font-weight" => parse_font_weight(value).is_some_and(|value| {
                self.font_weight = Some(value);
                true
            }),
            "color" => RgbaColor::parse(value).is_some_and(|value| {
                self.color = Some(value);
                true
            }),
            "line-height" => parse_line_height(value).is_some_and(|value| {
                self.line_height = Some(value);
                true
            }),
            "text-wrap" => parse_text_wrap(value).is_some_and(|value| {
                self.wrap = Some(value);
                true
            }),
            _ => false,
        }
    }

    /// Removes a declaration so that it inherits again.
    pub fn remove_property(&mut self, property: &str) -> bool {
        match property {
            "font-family" => self.font_family = None,
            "font-size" => self.font_size = None,
            "font-weight" => self.font_weight = None,
            "color" => self.color = None,
            "line-height" => self.line_height = None,
            "text-wrap" => self.wrap = None,
            _ => return false,
        }
        true
    }

    /// Resolves inherited declarations against a parent run or user-agent
    /// defaults. The resulting value is ready to translate into a shaper style.
    pub fn resolve(&self, parent: Option<&ComputedTextStyle>) -> ComputedTextStyle {
        let defaults;
        let parent = match parent {
            Some(parent) => parent,
            None => {
                defaults = ComputedTextStyle::default();
                &defaults
            }
        };

        ComputedTextStyle {
            font_family: self
                .font_family
                .clone()
                .unwrap_or_else(|| parent.font_family.clone()),
            font_size: self.font_size.unwrap_or(parent.font_size),
            font_weight: self.font_weight.unwrap_or(parent.font_weight),
            color: self.color.unwrap_or(parent.color),
            line_height: self.line_height.unwrap_or(parent.line_height),
            wrap: self.wrap.unwrap_or(parent.wrap),
        }
    }
}

/// Fully inherited typography for one shaped text run.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedTextStyle {
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub color: RgbaColor,
    pub line_height: LineHeight,
    pub wrap: TextWrap,
}

impl Default for ComputedTextStyle {
    fn default() -> Self {
        Self {
            font_family: "sans-serif".into(),
            font_size: 16.0,
            font_weight: FontWeight::NORMAL,
            color: RgbaColor::rgb(0, 0, 0),
            line_height: LineHeight::Normal,
            wrap: TextWrap::Wrap,
        }
    }
}

/// Box style plus inheritable typography for a styled `<text>` element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextElementStyle {
    pub common: CommonStyle,
    pub text: TextStyle,
}

impl Styles for TextElementStyle {
    fn to_taffy_style(&self) -> taffy::Style<String> {
        self.common.to_taffy_style()
    }

    fn supports_property(property: &str) -> bool {
        CommonStyle::supports_property(property) || TextStyle::supports_property(property)
    }

    fn set_property(&mut self, property: &str, value: &str) -> bool {
        if TextStyle::supports_property(property) {
            self.text.set_property(property, value)
        } else {
            self.common.set_property(property, value)
        }
    }

    fn remove_property(&mut self, property: &str) -> bool {
        if TextStyle::supports_property(property) {
            self.text.remove_property(property)
        } else {
            self.common.remove_property(property)
        }
    }
}

fn parse_font_family(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_font_weight(value: &str) -> Option<FontWeight> {
    match value.trim() {
        "normal" => Some(FontWeight::NORMAL),
        "bold" => Some(FontWeight::BOLD),
        value => value.parse::<u16>().ok().and_then(FontWeight::new),
    }
}

fn parse_line_height(value: &str) -> Option<LineHeight> {
    let value = value.trim();
    if value == "normal" {
        return Some(LineHeight::Normal);
    }
    if value.ends_with("px") {
        return parse_non_negative_px(value).map(LineHeight::Length);
    }
    parse_non_negative_finite(value).map(LineHeight::Factor)
}

fn parse_text_wrap(value: &str) -> Option<TextWrap> {
    match value.trim() {
        "wrap" => Some(TextWrap::Wrap),
        "nowrap" => Some(TextWrap::NoWrap),
        _ => None,
    }
}

fn parse_non_negative_px(value: &str) -> Option<f32> {
    parse_non_negative_finite(value.trim().strip_suffix("px")?)
}

fn parse_non_negative_finite(value: &str) -> Option<f32> {
    let value = value.trim().parse::<f32>().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_typography_properties() {
        let mut style = TextStyle::default();

        assert!(style.set_property("font-family", "Inter, sans-serif"));
        assert!(style.set_property("font-size", "18px"));
        assert!(style.set_property("font-weight", "bold"));
        assert!(style.set_property("color", "#1234"));
        assert!(style.set_property("line-height", "1.5"));
        assert!(style.set_property("text-wrap", "nowrap"));

        assert_eq!(style.font_family.as_deref(), Some("Inter, sans-serif"));
        assert_eq!(style.font_size, Some(18.0));
        assert_eq!(style.font_weight, Some(FontWeight::BOLD));
        assert_eq!(style.line_height, Some(LineHeight::Factor(1.5)));
        assert_eq!(style.wrap, Some(TextWrap::NoWrap));
    }

    #[test]
    fn rejects_invalid_typography_values() {
        let mut style = TextStyle::default();

        for (property, value) in [
            ("font-family", "  "),
            ("font-size", "12"),
            ("font-size", "-1px"),
            ("font-size", "NaNpx"),
            ("font-weight", "0"),
            ("font-weight", "1001"),
            ("color", "red"),
            ("line-height", "-1"),
            ("text-wrap", "balance"),
        ] {
            assert!(!style.set_property(property, value), "{property}: {value}");
        }
        assert_eq!(style, TextStyle::default());
    }

    #[test]
    fn resolves_omitted_properties_by_inheritance() {
        let parent = TextStyle {
            font_family: Some("Inter".into()),
            font_size: Some(20.0),
            color: Some(RgbaColor::rgb(1, 2, 3)),
            ..TextStyle::default()
        }
        .resolve(None);
        let child = TextStyle {
            font_weight: Some(FontWeight::BOLD),
            ..TextStyle::default()
        }
        .resolve(Some(&parent));

        assert_eq!(child.font_family, "Inter");
        assert_eq!(child.font_size, 20.0);
        assert_eq!(child.color, RgbaColor::rgb(1, 2, 3));
        assert_eq!(child.font_weight, FontWeight::BOLD);
        assert_eq!(child.wrap, TextWrap::Wrap);
    }

    #[test]
    fn removing_a_declaration_restores_inheritance() {
        let mut style = TextStyle {
            font_size: Some(24.0),
            ..TextStyle::default()
        };

        assert!(style.remove_property("font-size"));
        assert_eq!(style.font_size, None);
        assert!(!style.remove_property("unknown"));
    }
}
