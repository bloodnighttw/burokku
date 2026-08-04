use render::TextDecorationLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextDecorationLineValue(u8);

impl TextDecorationLineValue {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const UNDERLINE: Self = Self(1);
    pub(crate) const OVERLINE: Self = Self(2);
    pub(crate) const LINE_THROUGH: Self = Self(4);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl From<TextDecorationLineValue> for TextDecorationLine {
    fn from(value: TextDecorationLineValue) -> Self {
        let mut result = Self::NONE;
        for (source, target) in [
            (TextDecorationLineValue::UNDERLINE, Self::UNDERLINE),
            (TextDecorationLineValue::OVERLINE, Self::OVERLINE),
            (TextDecorationLineValue::LINE_THROUGH, Self::LINE_THROUGH),
        ] {
            if value.contains(source) {
                result = result.union(target);
            }
        }
        result
    }
}
