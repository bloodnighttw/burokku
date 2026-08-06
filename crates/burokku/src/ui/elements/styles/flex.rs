use taffy::{
    geometry::Size, AlignContent, AlignItems, AlignSelf, Dimension, FlexDirection, FlexWrap,
    JustifyContent, LengthPercentage,
};

/// The properties used to lay out a flex container and its flex items.
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

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            gap: Size {
                width: LengthPercentage::length(0.0),
                height: LengthPercentage::length(0.0),
            },
            align_content: None,
            align_items: None,
            justify_content: None,
            basis: Dimension::auto(),
            grow: 0.0,
            shrink: 1.0,
            align_self: None,
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
        assert_eq!(flex.gap, taffy.gap);
        assert_eq!(flex.align_content, taffy.align_content);
        assert_eq!(flex.align_items, taffy.align_items);
        assert_eq!(flex.justify_content, taffy.justify_content);
        assert_eq!(flex.basis, taffy.flex_basis);
        assert_eq!(flex.grow, taffy.flex_grow);
        assert_eq!(flex.shrink, taffy.flex_shrink);
        assert_eq!(flex.align_self, taffy.align_self);
    }
}
