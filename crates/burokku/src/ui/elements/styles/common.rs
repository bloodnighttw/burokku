//! Common style properties shared by regular layout elements.

use taffy::geometry::{Rect, Size};

use crate::ui::elements::{
    styles::{
        color::RgbaColor,
        item::ItemStyle,
        length::{
            parse_length_percentage, parse_non_negative_dimension,
            parse_non_negative_length_percentage, to_taffy_auto, Dimension, LengthPercentage,
        },
    },
    traits::Styles,
};

/// Layout and paint properties shared by regular layout elements.
#[derive(Clone, Debug, PartialEq)]
pub struct CommonStyle {
    pub size: Size<Dimension>,
    pub padding: Rect<LengthPercentage>,
    pub margin: Rect<LengthPercentage>,
    pub background_color: Option<RgbaColor>,
    pub item: ItemStyle,
}

impl Styles for CommonStyle {
    fn to_taffy_style(&self) -> taffy::Style<String> {
        let mut style = taffy::Style {
            // Divs use CommonStyle directly and must not inherit Taffy's Flex default.
            // FlexStyle and GridStyle explicitly override this container display mode.
            display: taffy::Display::Block,
            size: Size {
                width: self.size.width.to_taffy(),
                height: self.size.height.to_taffy(),
            },
            padding: Rect {
                left: self.padding.left.to_taffy(),
                right: self.padding.right.to_taffy(),
                top: self.padding.top.to_taffy(),
                bottom: self.padding.bottom.to_taffy(),
            },
            margin: Rect {
                left: to_taffy_auto(self.margin.left),
                right: to_taffy_auto(self.margin.right),
                top: to_taffy_auto(self.margin.top),
                bottom: to_taffy_auto(self.margin.bottom),
            },
            ..Default::default()
        };
        self.item.clone().apply_to_taffy(&mut style);
        style
    }

    fn supports_property(property: &str) -> bool {
        matches!(
            property,
            "width" | "height" | "padding" | "margin" | "background-color"
        ) || ItemStyle::supports_property(property)
    }

    fn set_property(&mut self, property: &str, value: &str) -> bool {
        match property {
            "width" => parse_non_negative_dimension(value).is_some_and(|value| {
                self.size.width = value;
                true
            }),
            "height" => parse_non_negative_dimension(value).is_some_and(|value| {
                self.size.height = value;
                true
            }),
            "padding" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.padding = Rect {
                    left: value,
                    right: value,
                    top: value,
                    bottom: value,
                };
                true
            }),
            "margin" => parse_length_percentage(value).is_some_and(|value| {
                self.margin = Rect {
                    left: value,
                    right: value,
                    top: value,
                    bottom: value,
                };
                true
            }),
            "background-color" => RgbaColor::parse(value).is_some_and(|value| {
                self.background_color = Some(value);
                true
            }),
            _ => self.item.set_property(property, value),
        }
    }

    fn remove_property(&mut self, property: &str) -> bool {
        let defaults = Self::default();
        match property {
            "width" => self.size.width = defaults.size.width,
            "height" => self.size.height = defaults.size.height,
            "padding" => self.padding = defaults.padding,
            "margin" => self.margin = defaults.margin,
            "background-color" => self.background_color = defaults.background_color,
            _ => return self.item.remove_property(property),
        };
        true
    }
}

impl Default for CommonStyle {
    fn default() -> Self {
        Self {
            size: Size {
                width: Dimension::Auto,
                height: Dimension::Auto,
            },
            padding: Rect {
                left: LengthPercentage::ZERO,
                right: LengthPercentage::ZERO,
                top: LengthPercentage::ZERO,
                bottom: LengthPercentage::ZERO,
            },
            margin: Rect {
                left: LengthPercentage::ZERO,
                right: LengthPercentage::ZERO,
                top: LengthPercentage::ZERO,
                bottom: LengthPercentage::ZERO,
            },
            background_color: None,
            item: ItemStyle::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_convert_to_taffy_block_display() {
        let style = CommonStyle::default().to_taffy_style();

        assert_eq!(style.display, taffy::Display::Block);
    }
}
