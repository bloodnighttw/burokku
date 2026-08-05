pub use render::{
    Color, FontFamily, FontStyle, TextAlign, TextDecorationLine, TextOverflowWrap, TextShadow,
    TextWhiteSpace, TextWordBreak, TextWrap, Transform,
};

/// The typography and paint properties used to lay out and render text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    pub font_size: f32,
    pub line_height: f32,
    /// Whether the computed line height came from CSS `normal`.
    pub line_height_is_normal: bool,
    /// OpenType/CSS-like weight, where `400` is normal and `700` is bold.
    pub font_weight: u16,
    pub font_families: Vec<FontFamily>,
    pub font_style: FontStyle,
    pub text_align: TextAlign,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_decoration_line: TextDecorationLine,
    pub text_decoration_color: Color,
    /// Whether the decoration color follows [`TextStyle::color`].
    pub text_decoration_color_is_current: bool,
    pub white_space: TextWhiteSpace,
    pub overflow_wrap: TextOverflowWrap,
    pub word_break: TextWordBreak,
    pub wrap: TextWrap,
    pub opacity: f32,
    pub transform: Transform,
    pub shadows: Vec<TextShadow>,
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
            line_height: style.line_height,
            line_height_is_normal: style.line_height_is_normal,
            font_weight: style.font_weight,
            font_families: style.font_families,
            font_style: style.font_style,
            text_align: style.text_align,
            letter_spacing: style.letter_spacing,
            word_spacing: style.word_spacing,
            text_decoration_line: style.text_decoration_line,
            text_decoration_color: style.text_decoration_color,
            text_decoration_color_is_current: style.text_decoration_color_is_current,
            white_space: style.white_space,
            overflow_wrap: style.overflow_wrap,
            word_break: style.word_break,
            wrap: style.wrap,
            opacity: style.opacity,
            transform: style.transform,
            shadows: style.shadows,
        }
    }
}

impl From<TextStyle> for render::TextStyle {
    fn from(style: TextStyle) -> Self {
        Self {
            color: style.color,
            font_size: style.font_size,
            line_height: style.line_height,
            line_height_is_normal: style.line_height_is_normal,
            font_weight: style.font_weight,
            font_families: style.font_families,
            font_style: style.font_style,
            text_align: style.text_align,
            letter_spacing: style.letter_spacing,
            word_spacing: style.word_spacing,
            text_decoration_line: style.text_decoration_line,
            text_decoration_color: style.text_decoration_color,
            text_decoration_color_is_current: style.text_decoration_color_is_current,
            white_space: style.white_space,
            overflow_wrap: style.overflow_wrap,
            word_break: style.word_break,
            wrap: style.wrap,
            opacity: style.opacity,
            transform: style.transform,
            shadows: style.shadows,
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
    fn render_style_round_trips_without_losing_properties() {
        let expected = render::TextStyle {
            color: Color::WHITE,
            font_size: 20.0,
            line_height: 28.0,
            font_weight: 700,
            text_align: TextAlign::Center,
            letter_spacing: 1.5,
            opacity: 0.75,
            ..render::TextStyle::default()
        };

        let actual = render::TextStyle::from(TextStyle::from(expected.clone()));

        assert_eq!(actual, expected);
    }
}
