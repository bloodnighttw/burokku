pub trait Styles {
    // this is for converting to taffy style to calculate layout
    fn to_taffy_style(self) -> taffy::Style<String>;

    // return true when the property is recognized by this style type
    fn supports_property(property: &str) -> bool;

    // return false when property is not recognized or value is invalid
    fn set_property(&mut self, property: &str, value: &str) -> bool;

    // return false when property is not recognized
    fn remove_property(&mut self, property: &str) -> bool;
}

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