//! Common style properties shared by regular layout elements.

use taffy::geometry::{Rect, Size};

use crate::ui::elements::{
    styles::{
        color::RgbaColor,
        item::ItemStyle,
        length::{
            parse_dimension, parse_length_percentage, parse_non_negative_dimension,
            parse_non_negative_length_percentage, to_taffy_auto, Dimension, LengthPercentage,
        },
        position::Position,
    },
    traits::Styles,
};

/// Layout and paint properties shared by regular layout elements.
#[derive(Clone, Debug, PartialEq)]
pub struct CommonStyle {
    pub position: Position,
    pub inset: Rect<Dimension>,
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
            position: self.position.into(),
            inset: if self.position == Position::Static {
                Rect::auto()
            } else {
                self.inset.map(Dimension::to_taffy_auto)
            },
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
            "position"
                | "top"
                | "right"
                | "bottom"
                | "left"
                | "width"
                | "height"
                | "padding"
                | "margin"
                | "background-color"
        ) || ItemStyle::supports_property(property)
    }

    fn set_property(&mut self, property: &str, value: &str) -> bool {
        match property {
            "position" => Position::parse(value).is_some_and(|value| {
                self.position = value;
                true
            }),
            "top" => parse_dimension(value).is_some_and(|value| {
                self.inset.top = value;
                true
            }),
            "right" => parse_dimension(value).is_some_and(|value| {
                self.inset.right = value;
                true
            }),
            "bottom" => parse_dimension(value).is_some_and(|value| {
                self.inset.bottom = value;
                true
            }),
            "left" => parse_dimension(value).is_some_and(|value| {
                self.inset.left = value;
                true
            }),
            "width" => parse_non_negative_dimension(value).is_some_and(|value| {
                self.size.width = value;
                true
            }),
            "height" => parse_non_negative_dimension(value).is_some_and(|value| {
                self.size.height = value;
                true
            }),
            "padding" => parse_padding(value).is_some_and(|value| {
                self.padding = value;
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
            "position" => self.position = defaults.position,
            "top" => self.inset.top = defaults.inset.top,
            "right" => self.inset.right = defaults.inset.right,
            "bottom" => self.inset.bottom = defaults.inset.bottom,
            "left" => self.inset.left = defaults.inset.left,
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

fn parse_padding(value: &str) -> Option<Rect<LengthPercentage>> {
    let values = value
        .split_ascii_whitespace()
        .map(parse_non_negative_length_percentage)
        .collect::<Option<Vec<_>>>()?;

    match values.as_slice() {
        [all] => Some(Rect {
            left: *all,
            right: *all,
            top: *all,
            bottom: *all,
        }),
        [vertical, horizontal] => Some(Rect {
            left: *horizontal,
            right: *horizontal,
            top: *vertical,
            bottom: *vertical,
        }),
        [top, horizontal, bottom] => Some(Rect {
            left: *horizontal,
            right: *horizontal,
            top: *top,
            bottom: *bottom,
        }),
        [top, right, bottom, left] => Some(Rect {
            left: *left,
            right: *right,
            top: *top,
            bottom: *bottom,
        }),
        _ => None,
    }
}

impl Default for CommonStyle {
    fn default() -> Self {
        Self {
            position: Position::Static,
            inset: Rect {
                left: Dimension::Auto,
                right: Dimension::Auto,
                top: Dimension::Auto,
                bottom: Dimension::Auto,
            },
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
        assert_eq!(style.position, taffy::Position::Relative);
    }

    #[test]
    fn position_sets_converts_and_removes() {
        let mut style = CommonStyle::default();

        assert!(CommonStyle::supports_property("position"));
        assert!(style.set_property("position", "fixed"));
        assert_eq!(style.position, Position::Fixed);
        assert_eq!(style.to_taffy_style().position, taffy::Position::Absolute);
        assert!(!style.set_property("position", "sticky"));
        assert!(style.remove_property("position"));
        assert_eq!(style.position, Position::Static);
    }

    #[test]
    fn static_ignores_insets_until_positioned() {
        let mut style = CommonStyle::default();
        assert!(style.set_property("top", "-10px"));
        assert!(style.set_property("right", "25%"));
        assert_eq!(style.to_taffy_style().inset, Rect::auto());

        assert!(style.set_property("position", "absolute"));
        let inset = style.to_taffy_style().inset;
        assert_eq!(inset.top, taffy::LengthPercentageAuto::length(-10.0));
        assert_eq!(inset.right, taffy::LengthPercentageAuto::percent(0.25));
        assert_eq!(inset.bottom, taffy::LengthPercentageAuto::auto());
        assert_eq!(inset.left, taffy::LengthPercentageAuto::auto());
    }

    #[test]
    fn padding_shorthand_expands_one_to_four_values() {
        let px = LengthPercentage::length;
        let percent = LengthPercentage::percent;

        assert_eq!(
            parse_padding("5px"),
            Some(Rect {
                left: px(5.0),
                right: px(5.0),
                top: px(5.0),
                bottom: px(5.0),
            })
        );
        assert_eq!(
            parse_padding("50% 20%"),
            Some(Rect {
                left: percent(0.2),
                right: percent(0.2),
                top: percent(0.5),
                bottom: percent(0.5),
            })
        );
        assert_eq!(
            parse_padding("1px 2px 3px"),
            Some(Rect {
                left: px(2.0),
                right: px(2.0),
                top: px(1.0),
                bottom: px(3.0),
            })
        );
        assert_eq!(
            parse_padding("1px 2px 3px 4px"),
            Some(Rect {
                left: px(4.0),
                right: px(2.0),
                top: px(1.0),
                bottom: px(3.0),
            })
        );
        assert_eq!(parse_padding("1px 2px 3px 4px 5px"), None);
    }
}
