//! this module defines the common style properties shared by block, flex, and grid elements.

use taffy::{
    geometry::{Rect, Size},
    AlignSelf,
};

use crate::ui::elements::styles::{Dimension, LengthPercentage, RgbaColor, parse_dimension, parse_length_percentage, to_taffy_auto};

// Layout and paint properties shared by block, flex, and grid elements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommonStyle {
    pub size: Size<Dimension>,
    pub padding: Rect<LengthPercentage>,
    pub margin: Rect<LengthPercentage>,
    pub background_color: Option<RgbaColor>,
    pub flex_basis: Dimension,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub align_self: Option<AlignSelf>,
}

impl CommonStyle {
    pub(crate) fn supports(name: &str) -> bool {
        matches!(
            name,
            "width"
                | "height"
                | "padding"
                | "margin"
                | "background-color"
                | "flex-basis"
                | "flex-grow"
                | "flex-shrink"
                | "align-self"
        )
    }

    pub(crate) fn set_property(&mut self, name: &str, value: &str) -> bool {
        match name {
            "width" => parse_dimension(value).is_some_and(|value| {
                self.size.width = value;
                true
            }),
            "height" => parse_dimension(value).is_some_and(|value| {
                self.size.height = value;
                true
            }),
            "padding" => parse_length_percentage(value).is_some_and(|value| {
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
            "flex-basis" => parse_dimension(value).is_some_and(|value| {
                self.flex_basis = value;
                true
            }),
            "flex-grow" => value.parse().ok().is_some_and(|value| {
                self.flex_grow = value;
                true
            }),
            "flex-shrink" => value.parse().ok().is_some_and(|value| {
                self.flex_shrink = value;
                true
            }),
            "align-self" => value.parse().ok().is_some_and(|value| {
                self.align_self = Some(value);
                true
            }),
            _ => false,
        }
    }

    pub(crate) fn remove_property(&mut self, name: &str) -> bool {
        if !Self::supports(name) {
            return false;
        }
        let defaults = Self::default();
        match name {
            "width" => self.size.width = defaults.size.width,
            "height" => self.size.height = defaults.size.height,
            "padding" => self.padding = defaults.padding,
            "margin" => self.margin = defaults.margin,
            "background-color" => self.background_color = defaults.background_color,
            "flex-basis" => self.flex_basis = defaults.flex_basis,
            "flex-grow" => self.flex_grow = defaults.flex_grow,
            "flex-shrink" => self.flex_shrink = defaults.flex_shrink,
            "align-self" => self.align_self = defaults.align_self,
            _ => unreachable!("support was checked above"),
        }
        true
    }

    pub(crate) fn apply_to_taffy(self, style: &mut taffy::Style<String>) {
        style.size = Size {
            width: self.size.width.to_taffy(),
            height: self.size.height.to_taffy(),
        };
        style.padding = Rect {
            left: self.padding.left.to_taffy(),
            right: self.padding.right.to_taffy(),
            top: self.padding.top.to_taffy(),
            bottom: self.padding.bottom.to_taffy(),
        };
        style.margin = Rect {
            left: to_taffy_auto(self.margin.left),
            right: to_taffy_auto(self.margin.right),
            top: to_taffy_auto(self.margin.top),
            bottom: to_taffy_auto(self.margin.bottom),
        };
        style.flex_basis = self.flex_basis.to_taffy();
        style.flex_grow = self.flex_grow;
        style.flex_shrink = self.flex_shrink;
        style.align_self = self.align_self;
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
            flex_basis: Dimension::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            align_self: None,
        }
    }
}