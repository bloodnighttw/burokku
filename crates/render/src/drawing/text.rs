use super::Color;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontFamily {
    #[default]
    SansSerif,
    Serif,
    Monospace,
    Cursive,
    Fantasy,
    Named(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextWrap {
    None,
    Glyph,
    #[default]
    Word,
    WordOrGlyph,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextWhiteSpace {
    #[default]
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextOverflowWrap {
    #[default]
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextWordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextDecorationLine(u8);

impl TextDecorationLine {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1);
    pub const OVERLINE: Self = Self(2);
    pub const LINE_THROUGH: Self = Self(4);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub color: Color,
    pub font_size: f32,
    pub line_height: f32,
    /// Tracks whether the computed line height came from CSS `normal`.
    pub line_height_is_normal: bool,
    /// OpenType/CSS-like weight: 400 is normal and 700 is bold.
    pub font_weight: u16,
    pub font_families: Vec<FontFamily>,
    pub font_style: FontStyle,
    pub text_align: TextAlign,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub text_decoration_line: TextDecorationLine,
    pub text_decoration_color: Color,
    pub text_decoration_color_is_current: bool,
    pub white_space: TextWhiteSpace,
    pub overflow_wrap: TextOverflowWrap,
    pub word_break: TextWordBreak,
    pub wrap: TextWrap,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            font_size: 16.0,
            line_height: 19.2,
            line_height_is_normal: true,
            font_weight: 400,
            font_families: vec![FontFamily::SansSerif],
            font_style: FontStyle::Normal,
            text_align: TextAlign::Start,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_decoration_line: TextDecorationLine::NONE,
            text_decoration_color: Color::BLACK,
            text_decoration_color_is_current: true,
            white_space: TextWhiteSpace::Normal,
            overflow_wrap: TextOverflowWrap::Normal,
            word_break: TextWordBreak::Normal,
            wrap: TextWrap::Word,
        }
    }
}
