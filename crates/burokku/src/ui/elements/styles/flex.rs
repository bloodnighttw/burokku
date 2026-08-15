use taffy::{
    geometry::Size, AlignContent, AlignItems, AlignSelf, FlexDirection, FlexWrap, JustifyContent,
};

use super::{Dimension, LengthPercentage};

/// Thread-safe properties used to lay out a flex container and its flex items.
///
/// This is authoritative DOM data and contains no Taffy compact pointer values.
/// MTS converts it to [`taffy::Style`] immediately before updating layout state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexStyle {
    // Container properties
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub gap: Size<LengthPercentage>,
    pub align_content: Option<AlignContent>,
    pub align_items: Option<AlignItems>,
    pub justify_content: Option<JustifyContent>,

    // Item properties
    pub basis: Dimension,
    pub grow: f32,
    pub shrink: f32,
    pub align_self: Option<AlignSelf>,
}

impl FlexStyle {
    /// Build the Taffy value consumed only by MTS computed/layout state.
    pub fn to_taffy_style(self) -> taffy::Style<String> {
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
            flex_basis: self.basis.to_taffy(),
            flex_grow: self.grow,
            flex_shrink: self.shrink,
            align_self: self.align_self,
            ..taffy::Style::default()
        }
    }
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            gap: Size {
                width: LengthPercentage::ZERO,
                height: LengthPercentage::ZERO,
            },
            align_content: None,
            align_items: None,
            justify_content: None,
            basis: Dimension::Auto,
            grow: 0.0,
            shrink: 1.0,
            align_self: None,
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
            basis: Dimension::percent(0.5),
            grow: 2.0,
            ..FlexStyle::default()
        };
        let taffy = style.to_taffy_style();

        assert_eq!(taffy.gap.width, taffy::LengthPercentage::length(8.0));
        assert_eq!(taffy.gap.height, taffy::LengthPercentage::percent(0.1));
        assert_eq!(taffy.flex_basis, taffy::Dimension::percent(0.5));
        assert_eq!(taffy.flex_grow, 2.0);
    }
}
