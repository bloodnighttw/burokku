/// The supported values of CSS `isolation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Isolation {
    #[default]
    Auto,
    Isolate,
}

impl From<Isolation> for bool {
    fn from(value: Isolation) -> Self {
        value == Isolation::Isolate
    }
}
