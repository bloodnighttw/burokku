use taffy::style::Position as TaffyPosition;

/// The supported values of CSS `position`.
///
/// This remains distinct from Taffy's two-value position type so paint and
/// stacking code can distinguish fixed boxes from absolute boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
}

impl Position {
    pub(crate) const fn is_positioned(self) -> bool {
        !matches!(self, Self::Static)
    }
}

impl From<Position> for TaffyPosition {
    fn from(value: Position) -> Self {
        match value {
            Position::Static | Position::Relative => Self::Relative,
            Position::Absolute | Position::Fixed => Self::Absolute,
        }
    }
}
