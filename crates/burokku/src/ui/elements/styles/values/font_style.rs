use render::FontStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontStyleValue {
    Normal,
    Italic,
    Oblique,
}

impl From<FontStyleValue> for FontStyle {
    fn from(value: FontStyleValue) -> Self {
        match value {
            FontStyleValue::Normal => Self::Normal,
            FontStyleValue::Italic => Self::Italic,
            FontStyleValue::Oblique => Self::Oblique,
        }
    }
}
