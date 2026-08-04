/// The supported forms of CSS `line-height`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) enum LineHeightValue {
    Normal,
    Number(f32),
    Px(f32),
    Percent(f32),
}
