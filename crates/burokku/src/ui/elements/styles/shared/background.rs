pub use render::{BackgroundImage, Color};

/// Background paint shared by box-like element styles.
#[derive(Clone, Debug, PartialEq)]
pub struct Background {
    pub color: Color,
    pub image: Option<BackgroundImage>,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            color: Color::TRANSPARENT,
            image: None,
        }
    }
}

/// Provides access to a style's background properties.
pub trait BackgroundStyle {
    fn background(&self) -> &Background;

    fn background_mut(&mut self) -> &mut Background;
}

/// Implements [`BackgroundStyle`] for a type with a `background` field.
///
/// Keeping the field as a composed value lets other independent shared style
/// groups, such as borders and corner radii, use the same pattern on one type.
macro_rules! impl_background_style {
    ($style:ty) => {
        impl $crate::ui::elements::styles::shared::background::BackgroundStyle for $style {
            fn background(&self) -> &$crate::ui::elements::styles::shared::background::Background {
                &self.background
            }

            fn background_mut(
                &mut self,
            ) -> &mut $crate::ui::elements::styles::shared::background::Background {
                &mut self.background
            }
        }
    };
}

pub(crate) use impl_background_style;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_render_box_background_defaults() {
        let background = Background::default();
        let render_style = render::BoxStyle::default();

        assert_eq!(background.color, render_style.background);
        assert_eq!(background.image, render_style.background_image);
    }
}
