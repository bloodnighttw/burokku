//! Layout properties that describe an element's relationship to its parent.

use taffy::{geometry::Line, AlignSelf, GridPlacement as TaffyGridPlacement, JustifySelf};

use crate::ui::elements::traits::IntoTaffyStyle;

/// A thread-safe grid line placement.
///
/// Named lines use owned strings so the value remains safe to keep in the
/// authoritative DOM and move across threads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GridPlacement {
    #[default]
    Auto,
    Line(i16),
    NamedLine(String, i16),
    Span(u16),
    NamedSpan(String, u16),
}

impl IntoTaffyStyle for GridPlacement {
    type Into = TaffyGridPlacement<String>;

    fn into_taffy_style(self) -> Self::Into {
        match self {
            Self::Auto => TaffyGridPlacement::Auto,
            Self::Line(index) => TaffyGridPlacement::Line(index.into()),
            Self::NamedLine(name, index) => TaffyGridPlacement::NamedLine(name, index),
            Self::Span(tracks) => TaffyGridPlacement::Span(tracks),
            Self::NamedSpan(name, occurrence) => TaffyGridPlacement::NamedSpan(name, occurrence),
        }
    }
}

/// Item properties shared by every regular layout element.
///
/// Flex and grid placement belong to the child, not to the container. Keeping
/// these values in the shared style lets a div, flex, grid, or text element be
/// positioned by whichever container it is inserted into.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemStyle {
    pub flex_basis: super::length::Dimension,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub align_self: Option<AlignSelf>,
    pub grid_row: Line<GridPlacement>,
    pub grid_column: Line<GridPlacement>,
    pub justify_self: Option<JustifySelf>,
}

impl ItemStyle {
    pub(crate) fn apply_to_taffy(self, style: &mut taffy::Style<String>) {
        style.flex_basis = self.flex_basis.to_taffy();
        style.flex_grow = self.flex_grow;
        style.flex_shrink = self.flex_shrink;
        style.align_self = self.align_self;
        style.grid_row = Line {
            start: self.grid_row.start.into_taffy_style(),
            end: self.grid_row.end.into_taffy_style(),
        };
        style.grid_column = Line {
            start: self.grid_column.start.into_taffy_style(),
            end: self.grid_column.end.into_taffy_style(),
        };
        style.justify_self = self.justify_self;
    }

    pub(crate) fn supports_property(property: &str) -> bool {
        matches!(
            property,
            "flex-basis"
                | "flex-grow"
                | "flex-shrink"
                | "align-self"
                | "grid-row"
                | "grid-row-start"
                | "grid-row-end"
                | "grid-column"
                | "grid-column-start"
                | "grid-column-end"
                | "justify-self"
        )
    }

    pub(crate) fn set_property(&mut self, property: &str, value: &str) -> bool {
        match property {
            "flex-basis" => {
                super::length::parse_non_negative_dimension(value).is_some_and(|value| {
                    self.flex_basis = value;
                    true
                })
            }
            "flex-grow" => parse_non_negative_finite(value).is_some_and(|value| {
                self.flex_grow = value;
                true
            }),
            "flex-shrink" => parse_non_negative_finite(value).is_some_and(|value| {
                self.flex_shrink = value;
                true
            }),
            "align-self" => parse_optional_alignment(value).is_some_and(|value| {
                self.align_self = value;
                true
            }),
            "grid-row" => parse_grid_line(value).is_some_and(|value| {
                self.grid_row = value;
                true
            }),
            "grid-row-start" => parse_grid_placement(value).is_some_and(|value| {
                self.grid_row.start = value;
                true
            }),
            "grid-row-end" => parse_grid_placement(value).is_some_and(|value| {
                self.grid_row.end = value;
                true
            }),
            "grid-column" => parse_grid_line(value).is_some_and(|value| {
                self.grid_column = value;
                true
            }),
            "grid-column-start" => parse_grid_placement(value).is_some_and(|value| {
                self.grid_column.start = value;
                true
            }),
            "grid-column-end" => parse_grid_placement(value).is_some_and(|value| {
                self.grid_column.end = value;
                true
            }),
            "justify-self" => parse_optional_alignment(value).is_some_and(|value| {
                self.justify_self = value;
                true
            }),
            _ => false,
        }
    }

    pub(crate) fn remove_property(&mut self, property: &str) -> bool {
        let defaults = Self::default();
        match property {
            "flex-basis" => self.flex_basis = defaults.flex_basis,
            "flex-grow" => self.flex_grow = defaults.flex_grow,
            "flex-shrink" => self.flex_shrink = defaults.flex_shrink,
            "align-self" => self.align_self = defaults.align_self,
            "grid-row" => self.grid_row = defaults.grid_row,
            "grid-row-start" => self.grid_row.start = defaults.grid_row.start,
            "grid-row-end" => self.grid_row.end = defaults.grid_row.end,
            "grid-column" => self.grid_column = defaults.grid_column,
            "grid-column-start" => self.grid_column.start = defaults.grid_column.start,
            "grid-column-end" => self.grid_column.end = defaults.grid_column.end,
            "justify-self" => self.justify_self = defaults.justify_self,
            _ => return false,
        }
        true
    }
}

impl Default for ItemStyle {
    fn default() -> Self {
        Self {
            flex_basis: super::length::Dimension::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            align_self: None,
            grid_row: Line {
                start: GridPlacement::Auto,
                end: GridPlacement::Auto,
            },
            grid_column: Line {
                start: GridPlacement::Auto,
                end: GridPlacement::Auto,
            },
            justify_self: None,
        }
    }
}

fn parse_non_negative_finite(value: &str) -> Option<f32> {
    let value = value.trim().parse::<f32>().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn parse_optional_alignment<T>(value: &str) -> Option<Option<T>>
where
    T: std::str::FromStr,
{
    if value.trim() == "auto" {
        Some(None)
    } else {
        value.trim().parse().ok().map(Some)
    }
}

fn parse_grid_placement(value: &str) -> Option<GridPlacement> {
    let placement = value.trim().parse::<TaffyGridPlacement<String>>().ok()?;
    Some(match placement {
        TaffyGridPlacement::Auto => GridPlacement::Auto,
        TaffyGridPlacement::Line(index) => GridPlacement::Line(index.as_i16()),
        TaffyGridPlacement::NamedLine(name, index) => GridPlacement::NamedLine(name, index),
        TaffyGridPlacement::Span(tracks) => GridPlacement::Span(tracks),
        TaffyGridPlacement::NamedSpan(name, occurrence) => {
            GridPlacement::NamedSpan(name, occurrence)
        }
    })
}

fn parse_grid_line(value: &str) -> Option<Line<GridPlacement>> {
    let mut placements = value.split('/');
    let start = parse_grid_placement(placements.next()?)?;
    let end = match placements.next() {
        Some(value) => parse_grid_placement(value)?,
        None => GridPlacement::Auto,
    };
    placements.next().is_none().then_some(Line { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grid_item_shorthands() {
        let mut style = ItemStyle::default();

        assert!(style.set_property("grid-row", "2 / span 3"));
        assert!(style.set_property("grid-column", "content 2 / span sidebar"));
        assert_eq!(
            style.grid_row,
            Line {
                start: GridPlacement::Line(2),
                end: GridPlacement::Span(3),
            }
        );
        assert_eq!(
            style.grid_column,
            Line {
                start: GridPlacement::NamedLine("content".into(), 2),
                end: GridPlacement::NamedSpan("sidebar".into(), 0),
            }
        );
    }

    #[test]
    fn conversion_applies_grid_item_properties() {
        let mut item = ItemStyle::default();
        assert!(item.set_property("grid-row", "2 / 4"));
        assert!(item.set_property("grid-column", "span 2"));
        assert!(item.set_property("justify-self", "center"));

        let mut taffy = taffy::Style::<String>::default();
        item.apply_to_taffy(&mut taffy);

        assert_eq!(
            taffy.grid_row,
            Line {
                start: TaffyGridPlacement::Line(2.into()),
                end: TaffyGridPlacement::Line(4.into()),
            }
        );
        assert_eq!(
            taffy.grid_column,
            Line {
                start: TaffyGridPlacement::Span(2),
                end: TaffyGridPlacement::Auto,
            }
        );
        assert_eq!(taffy.justify_self, Some(taffy::AlignItems::CENTER));
    }

    #[test]
    fn auto_clears_optional_item_alignment() {
        let mut style = ItemStyle::default();

        assert!(style.set_property("align-self", "center"));
        assert!(style.set_property("justify-self", "end"));
        assert!(style.set_property("align-self", "auto"));
        assert!(style.set_property("justify-self", "auto"));
        assert_eq!(style.align_self, None);
        assert_eq!(style.justify_self, None);
    }
}
