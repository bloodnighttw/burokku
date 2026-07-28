use taffy::style::Overflow as TaffyOverflow;

/// The supported forms of CSS overflow, preserving `auto` versus `scroll`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Auto,
    Scroll,
}

impl From<Overflow> for TaffyOverflow {
    fn from(value: Overflow) -> Self {
        match value {
            Overflow::Visible => Self::Visible,
            Overflow::Hidden => Self::Hidden,
            Overflow::Clip => Self::Clip,
            Overflow::Auto | Overflow::Scroll => Self::Scroll,
        }
    }
}
