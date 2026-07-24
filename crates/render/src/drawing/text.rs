use super::Color;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontFamily {
    #[default]
    SansSerif,
    Serif,
    Monospace,
    Named(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextWrap {
    None,
    Glyph,
    #[default]
    Word,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    pub font_size: f32,
    pub line_height: f32,
    /// OpenType/CSS-like weight: 400 is normal and 700 is bold.
    pub font_weight: u16,
    pub font_family: FontFamily,
    pub wrap: TextWrap,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            font_size: 16.0,
            line_height: 19.2,
            font_weight: 400,
            font_family: FontFamily::SansSerif,
            wrap: TextWrap::Word,
        }
    }
}
