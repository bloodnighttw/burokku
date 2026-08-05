pub use render::{
    Color, FontFamily, FontStyle, TextAlign, TextDecorationLine, TextOverflowWrap, TextWhiteSpace,
    TextWordBreak, TextWrap,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub enum LineHeight {
    #[default]
    Normal,
    Value(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextDecorationColor {
    #[default]
    Current,
    Color(Color),
}

/// The typography and paint properties used to lay out and render text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    pub font_size: f32,
    pub line_height: LineHeight,
    /// OpenType/CSS-like weight, where `400` is normal and `700` is bold.
    pub font_weight: u16,
    pub font_families: Vec<FontFamily>,
    pub font_style: FontStyle,
    pub text_align: TextAlign,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_decoration_line: TextDecorationLine,
    /// The decoration color, or [`TextDecorationColor::Current`] to follow
    /// [`TextStyle::color`].
    pub text_decoration_color: TextDecorationColor,
    pub white_space: TextWhiteSpace,
    pub overflow_wrap: TextOverflowWrap,
    pub word_break: TextWordBreak,
    pub wrap: TextWrap,
}

impl Default for TextStyle {
    fn default() -> Self {
        render::TextStyle::default().into()
    }
}

impl From<render::TextStyle> for TextStyle {
    fn from(style: render::TextStyle) -> Self {
        Self {
            color: style.color,
            font_size: style.font_size,
            line_height: if style.line_height_is_normal {
                LineHeight::Normal
            } else {
                LineHeight::Value(style.line_height)
            },
            font_weight: style.font_weight,
            font_families: style.font_families,
            font_style: style.font_style,
            text_align: style.text_align,
            letter_spacing: style.letter_spacing,
            word_spacing: style.word_spacing,
            text_decoration_line: style.text_decoration_line,
            text_decoration_color: if style.text_decoration_color_is_current {
                TextDecorationColor::Current
            } else {
                TextDecorationColor::Color(style.text_decoration_color)
            },
            white_space: style.white_space,
            overflow_wrap: style.overflow_wrap,
            word_break: style.word_break,
            wrap: style.wrap,
        }
    }
}

impl From<TextStyle> for render::TextStyle {
    fn from(style: TextStyle) -> Self {
        let (line_height, line_height_is_normal) = match style.line_height {
            LineHeight::Normal => (style.font_size * 1.2, true),
            LineHeight::Value(value) => (value, false),
        };

        let (text_decoration_color, text_decoration_color_is_current) =
            match style.text_decoration_color {
                TextDecorationColor::Current => (Color::BLACK, true),
                TextDecorationColor::Color(color) => (color, false),
            };

        Self {
            color: style.color,
            font_size: style.font_size,
            line_height,
            line_height_is_normal,
            font_weight: style.font_weight,
            font_families: style.font_families,
            font_style: style.font_style,
            text_align: style.text_align,
            letter_spacing: style.letter_spacing,
            word_spacing: style.word_spacing,
            text_decoration_line: style.text_decoration_line,
            text_decoration_color,
            text_decoration_color_is_current,
            white_space: style.white_space,
            overflow_wrap: style.overflow_wrap,
            word_break: style.word_break,
            wrap: style.wrap,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_render_text_defaults() {
        let text = TextStyle::default();
        let expected = render::TextStyle::default();

        assert_eq!(render::TextStyle::from(text), expected);
    }

    #[test]
    fn render_style_round_trips_without_losing_text_properties() {
        let expected = render::TextStyle {
            color: Color::WHITE,
            font_size: 20.0,
            line_height: 28.0,
            line_height_is_normal: false,
            font_weight: 700,
            text_align: TextAlign::Center,
            letter_spacing: 1.5,
            ..render::TextStyle::default()
        };

        let actual = render::TextStyle::from(TextStyle::from(expected.clone()));

        assert_eq!(actual, expected);
    }

    #[test]
    fn normal_line_height_scales_with_font_size() {
        let actual = render::TextStyle::from(TextStyle {
            font_size: 20.0,
            line_height: LineHeight::Normal,
            ..TextStyle::default()
        });

        assert_eq!(actual.line_height, 24.0);
        assert!(actual.line_height_is_normal);
    }

    #[test]
    fn explicit_decoration_color_does_not_follow_text_color() {
        let actual = render::TextStyle::from(TextStyle {
            color: Color::WHITE,
            text_decoration_color: TextDecorationColor::Color(Color::TRANSPARENT),
            ..TextStyle::default()
        });

        assert_eq!(actual.text_decoration_color, Color::TRANSPARENT);
        assert!(!actual.text_decoration_color_is_current);
    }
}
