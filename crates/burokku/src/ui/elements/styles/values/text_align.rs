use render::TextAlign;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextAlignValue {
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

impl From<TextAlignValue> for TextAlign {
    fn from(value: TextAlignValue) -> Self {
        match value {
            TextAlignValue::Start => Self::Start,
            TextAlignValue::End => Self::End,
            TextAlignValue::Left => Self::Left,
            TextAlignValue::Right => Self::Right,
            TextAlignValue::Center => Self::Center,
            TextAlignValue::Justify => Self::Justify,
        }
    }
}
