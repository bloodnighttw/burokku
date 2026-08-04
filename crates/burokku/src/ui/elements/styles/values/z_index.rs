/// The supported forms of CSS `z-index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ZIndex {
    #[default]
    Auto,
    Value(i32),
}

impl From<ZIndex> for Option<i32> {
    fn from(value: ZIndex) -> Self {
        match value {
            ZIndex::Auto => None,
            ZIndex::Value(value) => Some(value),
        }
    }
}
