pub use render::Border;

/// Provides access to a style's border properties.
pub trait BorderStyle {
    fn border(&self) -> &Option<Border>;

    fn border_mut(&mut self) -> &mut Option<Border>;
}

/// Implements [`BorderStyle`] for a type with a `border` field.
macro_rules! impl_border_style {
    ($style:ty) => {
        impl $crate::ui::elements::styles::shared::border::BorderStyle for $style {
            fn border(&self) -> &Option<$crate::ui::elements::styles::shared::border::Border> {
                &self.border
            }

            fn border_mut(
                &mut self,
            ) -> &mut Option<$crate::ui::elements::styles::shared::border::Border> {
                &mut self.border
            }
        }
    };
}

pub(crate) use impl_border_style;
