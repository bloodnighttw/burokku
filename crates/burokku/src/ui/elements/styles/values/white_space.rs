use render::TextWhiteSpace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhiteSpaceValue {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

impl From<WhiteSpaceValue> for TextWhiteSpace {
    fn from(value: WhiteSpaceValue) -> Self {
        match value {
            WhiteSpaceValue::Normal => Self::Normal,
            WhiteSpaceValue::NoWrap => Self::NoWrap,
            WhiteSpaceValue::Pre => Self::Pre,
            WhiteSpaceValue::PreWrap => Self::PreWrap,
            WhiteSpaceValue::PreLine => Self::PreLine,
            WhiteSpaceValue::BreakSpaces => Self::BreakSpaces,
        }
    }
}
