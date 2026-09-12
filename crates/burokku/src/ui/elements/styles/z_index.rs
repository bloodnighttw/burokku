#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ZIndex {
    #[default]
    Auto,
    Integer(i32),
}
