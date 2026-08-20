use crate::ui::elements::traits::Styles;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WindowSize {
    #[default]
    Auto,
    Fixed(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowStyle {
    pub width: WindowSize,
    pub height: WindowSize,
    pub background_color: Option<crate::ui::elements::styles::color::RgbaColor>,
}

fn parse_window_size(value: &str) -> Option<WindowSize> {
    if value == "auto" {
        Some(WindowSize::Auto)
    } else if let Ok(v) = value.parse::<f32>() {
        Some(WindowSize::Fixed(v))
    } else {
        None
    }
}

impl Styles for WindowStyle {
    fn to_taffy_style(self) -> taffy::Style<String> {
        taffy::Style {
            display: taffy::Display::Block,
            size: taffy::Size {
                width: match self.width {
                    WindowSize::Auto => taffy::Dimension::auto(),
                    WindowSize::Fixed(v) => taffy::Dimension::length(v),
                },
                height: match self.height {
                    WindowSize::Auto => taffy::Dimension::auto(),
                    WindowSize::Fixed(v) => taffy::Dimension::length(v),
                },
            },
            ..Default::default()
        }
    }

    fn supports_property(property: &str) -> bool {
        matches!(property, "width" | "height")
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
            "background-color" => {
                self.background_color = crate::ui::elements::styles::color::RgbaColor::parse(value);
                true
            }
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
            _ => false,
        }
    }
}
