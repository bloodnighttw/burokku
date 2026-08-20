use taffy::{geometry::Size, AlignContent, AlignItems, FlexDirection, FlexWrap, JustifyContent};

use crate::ui::elements::{styles::common::CommonStyle, traits::Styles};

#[cfg(test)]
use crate::ui::elements::styles::item::ItemStyle;

use super::length::{parse_non_negative_length_percentage, LengthPercentage};

/// Thread-safe properties used to lay out a flex container and its flex items.
///
/// This is authoritative DOM data and contains no Taffy compact pointer values.
/// MTS converts it to [`taffy::Style`] immediately before updating layout state.
#[derive(Clone, Debug, PartialEq)]
pub struct FlexStyle {
    pub common: CommonStyle,
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub gap: Size<LengthPercentage>,
    pub align_content: Option<AlignContent>,
    pub align_items: Option<AlignItems>,
    pub justify_content: Option<JustifyContent>,
}

impl Styles for FlexStyle {
    fn to_taffy_style(&self) -> taffy::Style<String> {
        taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: self.direction,
            flex_wrap: self.wrap,
            gap: Size {
                width: self.gap.width.to_taffy(),
                height: self.gap.height.to_taffy(),
            },
            align_content: self.align_content,
            align_items: self.align_items,
            justify_content: self.justify_content,
            ..self.common.to_taffy_style()
        }
    }

    fn supports_property(property: &str) -> bool {
        CommonStyle::supports_property(property)
            || matches!(
                property,
                "flex-direction"
                    | "flex-wrap"
                    | "gap"
                    | "column-gap"
                    | "row-gap"
                    | "align-content"
                    | "align-items"
                    | "justify-content"
            )
    }

    fn set_property(&mut self, property: &str, value: &str) -> bool {
        if self.common.set_property(property, value) {
            return true;
        }
        match property {
            "flex-direction" => value.parse().ok().is_some_and(|value| {
                self.direction = value;
                true
            }),
            "flex-wrap" => value.parse().ok().is_some_and(|value| {
                self.wrap = value;
                true
            }),
            "gap" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.gap = Size {
                    width: value,
                    height: value,
                };
                true
            }),
            "column-gap" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.gap.width = value;
                true
            }),
            "row-gap" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.gap.height = value;
                true
            }),
            "align-content" => value.parse().ok().is_some_and(|value| {
                self.align_content = Some(value);
                true
            }),
            "align-items" => value.parse().ok().is_some_and(|value| {
                self.align_items = Some(value);
                true
            }),
            "justify-content" => value.parse().ok().is_some_and(|value| {
                self.justify_content = Some(value);
                true
            }),
            _ => false,
        }
    }

    fn remove_property(&mut self, property: &str) -> bool {
        if self.common.remove_property(property) {
            return true;
        }
        let defaults = Self::default();
        match property {
            "flex-direction" => self.direction = defaults.direction,
            "flex-wrap" => self.wrap = defaults.wrap,
            "gap" => self.gap = defaults.gap,
            "column-gap" => self.gap.width = defaults.gap.width,
            "row-gap" => self.gap.height = defaults.gap.height,
            "align-content" => self.align_content = defaults.align_content,
            "align-items" => self.align_items = defaults.align_items,
            "justify-content" => self.justify_content = defaults.justify_content,
            _ => return false,
        }
        true
    }
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            common: CommonStyle::default(),
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            gap: Size {
                width: LengthPercentage::ZERO,
                height: LengthPercentage::ZERO,
            },
            align_content: None,
            align_items: None,
            justify_content: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn style_is_send_and_sync() {
        assert_send_sync::<FlexStyle>();
    }

    #[test]
    fn defaults_convert_to_taffy_flex_defaults() {
        let flex = FlexStyle::default().to_taffy_style();
        let expected = taffy::Style::<String> {
            display: taffy::Display::Flex,
            ..Default::default()
        };

        assert_eq!(flex, expected);
    }

    #[test]
    fn conversion_preserves_flex_values() {
        let style = FlexStyle {
            gap: Size {
                width: LengthPercentage::length(8.0),
                height: LengthPercentage::percent(0.1),
            },
            common: CommonStyle {
                item: ItemStyle {
                    flex_basis: crate::ui::elements::styles::length::Dimension::percent(0.5),
                    flex_grow: 2.0,
                    ..ItemStyle::default()
                },
                ..CommonStyle::default()
            },
            ..FlexStyle::default()
        };
        let taffy = style.to_taffy_style();

        assert_eq!(taffy.gap.width, taffy::LengthPercentage::length(8.0));
        assert_eq!(taffy.gap.height, taffy::LengthPercentage::percent(0.1));
        assert_eq!(taffy.flex_basis, taffy::Dimension::percent(0.5));
        assert_eq!(taffy.flex_grow, 2.0);
    }
}
