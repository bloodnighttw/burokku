use crate::ui::elements::{
    styles::{
        color::RgbaColor,
        length::{parse_non_negative_dimension, Dimension},
    },
    traits::Styles,
};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WindowSize {
    #[default]
    Auto,
    Fixed(f32),
    Percent(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowStyle {
    pub width: WindowSize,
    pub height: WindowSize,
    pub background_color: Option<RgbaColor>,
}

fn parse_window_size(value: &str) -> Option<WindowSize> {
    parse_non_negative_dimension(value).map(|value| match value {
        Dimension::Auto => WindowSize::Auto,
        Dimension::Length(value) => WindowSize::Fixed(value),
        Dimension::Percent(value) => WindowSize::Percent(value),
    })
}

impl Styles for WindowStyle {
    fn to_taffy_style(self) -> taffy::Style<String> {
        taffy::Style {
            display: taffy::Display::Block,
            size: taffy::Size {
                width: to_taffy_dimension(self.width),
                height: to_taffy_dimension(self.height),
            },
            ..Default::default()
        }
    }

    fn supports_property(property: &str) -> bool {
        matches!(property, "width" | "height" | "background-color")
    }

    fn set_property(&mut self, property: &str, value: &str) -> bool {
        match property {
            "width" => parse_window_size(value).is_some_and(|size| {
                self.width = size;
                true
            }),
            "height" => parse_window_size(value).is_some_and(|size| {
                self.height = size;
                true
            }),
            "background-color" => RgbaColor::parse(value).is_some_and(|color| {
                self.background_color = Some(color);
                true
            }),
            _ => false,
        }
    }

    fn remove_property(&mut self, property: &str) -> bool {
        match property {
            "width" => {
                self.width = WindowSize::Auto;
                true
            }
            "height" => {
                self.height = WindowSize::Auto;
                true
            }
            "background-color" => {
                self.background_color = None;
                true
            }
            _ => false,
        }
    }
}

fn to_taffy_dimension(value: WindowSize) -> taffy::Dimension {
    match value {
        WindowSize::Auto => taffy::Dimension::auto(),
        WindowSize::Fixed(value) => taffy::Dimension::length(value),
        WindowSize::Percent(value) => taffy::Dimension::percent(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_dimension_contract() {
        assert_eq!(parse_window_size("auto"), Some(WindowSize::Auto));
        assert_eq!(parse_window_size("640px"), Some(WindowSize::Fixed(640.0)));
        assert_eq!(parse_window_size("50%"), Some(WindowSize::Percent(0.5)));

        for invalid in ["640", "-1px", "NaNpx", "inf%"] {
            assert_eq!(parse_window_size(invalid), None, "{invalid}");
        }
    }
}
