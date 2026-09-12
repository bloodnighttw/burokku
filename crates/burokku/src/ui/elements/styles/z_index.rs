#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ZIndex {
    #[default]
    Auto,
    Integer(i32),
}

impl ZIndex {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value == "auto" {
            Some(Self::Auto)
        } else {
            value.parse().ok().map(Self::Integer)
        }
    }

    pub const fn level(self) -> i32 {
        match self {
            Self::Auto => 0,
            Self::Integer(value) => value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_auto_and_signed_integers() {
        assert_eq!(ZIndex::parse("auto"), Some(ZIndex::Auto));
        assert_eq!(ZIndex::parse("-12"), Some(ZIndex::Integer(-12)));
        assert_eq!(ZIndex::parse("7"), Some(ZIndex::Integer(7)));
        assert_eq!(ZIndex::parse("1.5"), None);
    }
}
