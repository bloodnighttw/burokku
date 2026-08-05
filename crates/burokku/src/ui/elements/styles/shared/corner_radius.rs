pub use render::CornerRadius;

/// Provides access to a style's corner-radius properties.
pub trait CornerRadiusStyle {
    fn corner_radius(&self) -> &CornerRadius;

    fn corner_radius_mut(&mut self) -> &mut CornerRadius;
}

/// Implements [`CornerRadiusStyle`] for a type with a `corner_radius` field.
macro_rules! impl_corner_radius_style {
    ($style:ty) => {
        impl $crate::ui::elements::styles::shared::corner_radius::CornerRadiusStyle for $style {
            fn corner_radius(
                &self,
            ) -> &$crate::ui::elements::styles::shared::corner_radius::CornerRadius {
                &self.corner_radius
            }

            fn corner_radius_mut(
                &mut self,
            ) -> &mut $crate::ui::elements::styles::shared::corner_radius::CornerRadius {
                &mut self.corner_radius
            }
        }
    };
}

pub(crate) use impl_corner_radius_style;
