use super::shared::{
    background::{impl_background_style, Background},
    border::{impl_border_style, Border},
    corner_radius::{impl_corner_radius_style, CornerRadius},
};

/// The paint properties used to render a block container.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DivStyle {
    pub background: Background,
    pub border: Option<Border>,
    pub corner_radius: CornerRadius,
}

impl_background_style!(DivStyle);
impl_border_style!(DivStyle);
impl_corner_radius_style!(DivStyle);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_render_box_defaults() {
        let div = DivStyle::default();
        let render_style = render::BoxStyle::default();

        assert_eq!(div.background.color, render_style.background);
        assert_eq!(div.background.image, render_style.background_image);
        assert_eq!(div.border, render_style.border);
        assert_eq!(div.corner_radius, render_style.corner_radius);
    }
}
