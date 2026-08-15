use super::elements::Elements;

/// Convert thread-safe authoritative DOM style data into Taffy data on MTS.
///
/// The returned value belongs to MTS computed state and is never published back
/// through the shared DOM snapshot.
#[allow(dead_code)] // Used when Phase 3 wires MTS computed state into the app.
pub fn taffy_style_for(element: &Elements) -> taffy::Style<String> {
    match element {
        Elements::Flex { style } => style.to_taffy_style(),
        Elements::Grid { style } => style.to_taffy_style(),
        Elements::App
        | Elements::Window
        | Elements::Div
        | Elements::Text
        | Elements::_String { .. } => taffy::Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::styles::{flex::FlexStyle, grid::GridStyle};

    #[test]
    fn mts_converts_authoritative_styles_for_taffy() {
        let flex = Elements::Flex {
            style: Box::new(FlexStyle {
                grow: 2.0,
                ..FlexStyle::default()
            }),
        };
        let grid = Elements::Grid {
            style: Box::default(),
        };

        let flex = taffy_style_for(&flex);
        let grid = taffy_style_for(&grid);

        assert_eq!(flex.display, taffy::Display::Flex);
        assert_eq!(flex.flex_grow, 2.0);
        assert_eq!(grid.display, taffy::Display::Grid);
        assert_eq!(grid, GridStyle::default().to_taffy_style());
    }
}
