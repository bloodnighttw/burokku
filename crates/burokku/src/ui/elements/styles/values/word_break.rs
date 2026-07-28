use render::TextWordBreak;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordBreakValue {
    Normal,
    BreakAll,
    KeepAll,
}

impl From<WordBreakValue> for TextWordBreak {
    fn from(value: WordBreakValue) -> Self {
        match value {
            WordBreakValue::Normal => Self::Normal,
            WordBreakValue::BreakAll => Self::BreakAll,
            WordBreakValue::KeepAll => Self::KeepAll,
        }
    }
}
