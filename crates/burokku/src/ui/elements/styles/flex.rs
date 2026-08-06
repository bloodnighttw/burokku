use taffy::{
    geometry::Size, AlignContent, AlignItems, AlignSelf, FlexDirection, FlexWrap, JustifyContent,
};

use super::shared::{
    background::{impl_background_style, Background},
    border::{impl_border_style, Border},
    corner_radius::{impl_corner_radius_style, CornerRadius},
};

/// The properties used to lay out a flex container and its flex items.
#[derive(Clone, Debug, PartialEq)]
pub struct FlexStyle {
    // Container properties
    pub direction: FlexDirection,
    pub wrap: FlexWrap,
    pub gap: Size<f32>,
    pub align_content: Option<AlignContent>,
    pub align_items: Option<AlignItems>,
    pub justify_content: Option<JustifyContent>,

    // Item properties
    pub basis: FlexBasis,
    pub grow: f32,
    pub shrink: f32,
    pub align_self: Option<AlignSelf>,

    // Shared paint properties
    pub background: Background,
    pub border: Option<Border>,
    pub corner_radius: CornerRadius,
}

/// The supported native flex-basis values.
///
/// This deliberately avoids storing Taffy's pointer-tagged `Dimension` in the
/// element tree, keeping [`FlexStyle`] safe to publish between the JavaScript
/// and window threads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FlexBasis {
    #[default]
    Auto,
    Length(f32),
}

impl_background_style!(FlexStyle);
impl_border_style!(FlexStyle);
impl_corner_radius_style!(FlexStyle);

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            gap: Size {
                width: 0.0,
                height: 0.0,
            },
            align_content: None,
            align_items: None,
            justify_content: None,
            basis: FlexBasis::Auto,
            grow: 0.0,
            shrink: 1.0,
            align_self: None,
            background: Background::default(),
            border: None,
            corner_radius: CornerRadius::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_taffy_flex_defaults() {
        let flex = FlexStyle::default();
        let taffy = taffy::Style::<String>::default();

        assert_eq!(flex.direction, taffy.flex_direction);
        assert_eq!(flex.wrap, taffy.flex_wrap);
        assert_eq!(flex.gap.width, 0.0);
        assert_eq!(flex.gap.height, 0.0);
        assert_eq!(flex.align_content, taffy.align_content);
        assert_eq!(flex.align_items, taffy.align_items);
        assert_eq!(flex.justify_content, taffy.justify_content);
        assert_eq!(flex.basis, FlexBasis::Auto);
        assert_eq!(flex.grow, taffy.flex_grow);
        assert_eq!(flex.shrink, taffy.flex_shrink);
        assert_eq!(flex.align_self, taffy.align_self);
        assert_eq!(flex.background, Background::default());
        assert_eq!(flex.border, None);
        assert_eq!(flex.corner_radius, CornerRadius::ZERO);
    }
}
