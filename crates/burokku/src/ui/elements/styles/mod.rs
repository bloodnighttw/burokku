use crate::ui::elements::styles::size_value::SizeValue;

mod measurement;
mod size_value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    display: taffy::Display,
    width: SizeValue,
    hight: SizeValue,
    min_width: Option<SizeValue>,
    min_hight: Option<SizeValue>,
    max_width: Option<SizeValue>,
    max_hight: Option<SizeValue>,
    
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: taffy::Display::Block,
            width: SizeValue::default(),
            hight: SizeValue::default(),
            min_hight: None,
            min_width: None,
            max_hight: None,
            max_width: None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Style;

    #[test]
    fn display_defaults_to_block() {
        assert_eq!(Style::default().display, taffy::Display::Block);
    }
}
