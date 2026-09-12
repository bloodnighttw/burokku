use taffy::Position as TaffyPosition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    // TODO: Sticky,
}

impl Position {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "static" => Some(Self::Static),
            "relative" => Some(Self::Relative),
            "absolute" => Some(Self::Absolute),
            "fixed" => Some(Self::Fixed),
            _ => None,
        }
    }
}

impl From<Position> for TaffyPosition {
    fn from(value: Position) -> Self {
        match value {
            Position::Static | Position::Relative => TaffyPosition::Relative,
            Position::Absolute | Position::Fixed => TaffyPosition::Absolute,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_lowers_supported_positions() {
        for (value, position, taffy) in [
            ("static", Position::Static, TaffyPosition::Relative),
            ("relative", Position::Relative, TaffyPosition::Relative),
            ("absolute", Position::Absolute, TaffyPosition::Absolute),
            ("fixed", Position::Fixed, TaffyPosition::Absolute),
        ] {
            assert_eq!(Position::parse(value), Some(position));
            assert_eq!(TaffyPosition::from(position), taffy);
        }
        assert_eq!(Position::parse("sticky"), None);
    }
}
