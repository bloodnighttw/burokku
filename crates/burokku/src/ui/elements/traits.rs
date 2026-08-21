//! some shared traits that used in ui/

pub trait Styles {
    // this is for converting to taffy style to calculate layout
    fn to_taffy_style(&self) -> taffy::Style<String>;

    // return true when the property is recognized by this style type
    fn supports_property(property: &str) -> bool;

    // return false when property is not recognized or value is invalid
    fn set_property(&mut self, property: &str, value: &str) -> bool;

    // return false when property is not recognized
    fn remove_property(&mut self, property: &str) -> bool;
}

/// this is for converting to Taffy style with custom types,
/// the difference from styles is that it is usually used for custom types of properties.
///
/// for example, grid properties type in taffy is not Send+Sync, which can't be
/// moved across threads, so we use IntoTaffyStyle to convert them to
/// custom types that have the same layout but are Send+Sync, then implement
/// this trait for those custom types to convert them into taffy style in main thread.
pub trait IntoTaffyStyle {
    type Into;

    fn into_taffy_style(self) -> Self::Into;
}

impl<T> IntoTaffyStyle for T
where
    T: Styles,
{
    type Into = taffy::Style<String>;

    fn into_taffy_style(self) -> Self::Into {
        self.to_taffy_style()
    }
}
