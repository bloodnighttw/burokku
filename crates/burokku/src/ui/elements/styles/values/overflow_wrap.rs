use render::TextOverflowWrap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowWrapValue {
    Normal,
    BreakWord,
    Anywhere,
}

impl From<OverflowWrapValue> for TextOverflowWrap {
    fn from(value: OverflowWrapValue) -> Self {
        match value {
            OverflowWrapValue::Normal => Self::Normal,
            OverflowWrapValue::BreakWord => Self::BreakWord,
            OverflowWrapValue::Anywhere => Self::Anywhere,
        }
    }
}
