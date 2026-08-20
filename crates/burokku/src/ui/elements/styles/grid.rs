use taffy::{
    geometry::{MinMax, Size},
    AlignContent, AlignItems, GridAutoFlow, JustifyContent, JustifyItems,
};

use crate::ui::elements::{
    styles::common::CommonStyle,
    traits::{IntoTaffyStyle, Styles},
};

use super::length::{parse_non_negative_length_percentage, LengthPercentage};

// Preserve the previous public path while item placement now lives in the
// shared item-style module.
pub use super::item::GridPlacement;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridTemplateArea {
    pub name: String,
    pub row_start: u16,
    pub row_end: u16,
    pub column_start: u16,
    pub column_end: u16,
}

impl IntoTaffyStyle for GridTemplateArea {
    type Into = taffy::GridTemplateArea<String>;

    fn into_taffy_style(self) -> Self::Into {
        taffy::GridTemplateArea {
            name: self.name,
            row_start: self.row_start,
            row_end: self.row_end,
            column_start: self.column_start,
            column_end: self.column_end,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MinTrackSizingFunction {
    Length(f32),
    Percent(f32),
    #[default]
    Auto,
    MinContent,
    MaxContent,
}

impl IntoTaffyStyle for MinTrackSizingFunction {
    type Into = taffy::MinTrackSizingFunction;

    fn into_taffy_style(self) -> taffy::MinTrackSizingFunction {
        match self {
            Self::Length(value) => taffy::MinTrackSizingFunction::length(value),
            Self::Percent(value) => taffy::MinTrackSizingFunction::percent(value),
            Self::Auto => taffy::MinTrackSizingFunction::auto(),
            Self::MinContent => taffy::MinTrackSizingFunction::min_content(),
            Self::MaxContent => taffy::MinTrackSizingFunction::max_content(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MaxTrackSizingFunction {
    Length(f32),
    Percent(f32),
    #[default]
    Auto,
    MinContent,
    MaxContent,
    FitContentLength(f32),
    FitContentPercent(f32),
    Fraction(f32),
}

impl IntoTaffyStyle for MaxTrackSizingFunction {
    type Into = taffy::MaxTrackSizingFunction;

    fn into_taffy_style(self) -> taffy::MaxTrackSizingFunction {
        match self {
            Self::Length(value) => taffy::MaxTrackSizingFunction::length(value),
            Self::Percent(value) => taffy::MaxTrackSizingFunction::percent(value),
            Self::Auto => taffy::MaxTrackSizingFunction::auto(),
            Self::MinContent => taffy::MaxTrackSizingFunction::min_content(),
            Self::MaxContent => taffy::MaxTrackSizingFunction::max_content(),
            Self::FitContentLength(value) => taffy::MaxTrackSizingFunction::fit_content_px(value),
            Self::FitContentPercent(value) => {
                taffy::MaxTrackSizingFunction::fit_content_percent(value)
            }
            Self::Fraction(value) => taffy::MaxTrackSizingFunction::fr(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSizingFunction {
    pub min: MinTrackSizingFunction,
    pub max: MaxTrackSizingFunction,
}

impl TrackSizingFunction {
    pub const AUTO: Self = Self {
        min: MinTrackSizingFunction::Auto,
        max: MaxTrackSizingFunction::Auto,
    };

    pub const fn length(value: f32) -> Self {
        Self {
            min: MinTrackSizingFunction::Length(value),
            max: MaxTrackSizingFunction::Length(value),
        }
    }

    pub const fn percent(value: f32) -> Self {
        Self {
            min: MinTrackSizingFunction::Percent(value),
            max: MaxTrackSizingFunction::Percent(value),
        }
    }

    pub const fn fraction(value: f32) -> Self {
        Self {
            min: MinTrackSizingFunction::Auto,
            max: MaxTrackSizingFunction::Fraction(value),
        }
    }

    fn to_taffy(self) -> taffy::TrackSizingFunction {
        MinMax {
            min: self.min.into_taffy_style(),
            max: self.max.into_taffy_style(),
        }
    }
}

impl Default for TrackSizingFunction {
    fn default() -> Self {
        Self::AUTO
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepetitionCount {
    AutoFill,
    AutoFit,
    Count(u16),
}

impl RepetitionCount {
    fn to_taffy(self) -> taffy::RepetitionCount {
        match self {
            Self::AutoFill => taffy::RepetitionCount::AutoFill,
            Self::AutoFit => taffy::RepetitionCount::AutoFit,
            Self::Count(count) => taffy::RepetitionCount::Count(count),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GridTemplateRepetition {
    pub count: RepetitionCount,
    pub tracks: Vec<TrackSizingFunction>,
    pub line_names: Vec<Vec<String>>,
}

impl IntoTaffyStyle for GridTemplateRepetition {
    type Into = taffy::GridTemplateRepetition<String>;

    fn into_taffy_style(self) -> taffy::GridTemplateRepetition<String> {
        taffy::GridTemplateRepetition {
            count: self.count.to_taffy(),
            tracks: self
                .tracks
                .iter()
                .copied()
                .map(TrackSizingFunction::to_taffy)
                .collect(),
            line_names: self.line_names.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GridTemplateComponent {
    Single(TrackSizingFunction),
    Repeat(GridTemplateRepetition),
}

impl IntoTaffyStyle for GridTemplateComponent {
    type Into = taffy::GridTemplateComponent<String>;

    fn into_taffy_style(self) -> taffy::GridTemplateComponent<String> {
        match self {
            Self::Single(track) => taffy::GridTemplateComponent::Single(track.to_taffy()),
            Self::Repeat(repetition) => {
                taffy::GridTemplateComponent::Repeat(repetition.clone().into_taffy_style())
            }
        }
    }
}

/// Thread-safe properties used to lay out a grid container.
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
    pub common: CommonStyle,

    // Container properties
    pub template_rows: Vec<GridTemplateComponent>,
    pub template_columns: Vec<GridTemplateComponent>,
    pub template_areas: Vec<GridTemplateArea>,
    pub template_row_names: Vec<Vec<String>>,
    pub template_column_names: Vec<Vec<String>>,
    pub auto_rows: Vec<TrackSizingFunction>,
    pub auto_columns: Vec<TrackSizingFunction>,
    pub auto_flow: GridAutoFlow,
    pub gap: Size<LengthPercentage>,
    pub align_content: Option<AlignContent>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub justify_items: Option<JustifyItems>,
}

impl Styles for GridStyle {
    fn to_taffy_style(&self) -> taffy::Style<String> {
        taffy::Style {
            display: taffy::Display::Grid,
            grid_template_rows: self
                .template_rows
                .clone()
                .into_iter()
                .map(IntoTaffyStyle::into_taffy_style)
                .collect(),
            grid_template_columns: self
                .template_columns
                .clone()
                .into_iter()
                .map(IntoTaffyStyle::into_taffy_style)
                .collect(),
            grid_template_areas: self
                .template_areas
                .clone()
                .into_iter()
                .map(IntoTaffyStyle::into_taffy_style)
                .collect(),
            grid_template_row_names: self.template_row_names.clone(),
            grid_template_column_names: self.template_column_names.clone(),
            grid_auto_rows: self
                .auto_rows
                .clone()
                .into_iter()
                .map(TrackSizingFunction::to_taffy)
                .collect(),
            grid_auto_columns: self
                .auto_columns
                .clone()
                .into_iter()
                .map(TrackSizingFunction::to_taffy)
                .collect(),
            grid_auto_flow: self.auto_flow,
            gap: Size {
                width: self.gap.width.to_taffy(),
                height: self.gap.height.to_taffy(),
            },
            align_content: self.align_content,
            justify_content: self.justify_content,
            align_items: self.align_items,
            justify_items: self.justify_items,
            ..self.common.to_taffy_style()
        }
    }

    fn supports_property(property: &str) -> bool {
        CommonStyle::supports_property(property)
            || matches!(
                property,
                "gap"
                    | "column-gap"
                    | "row-gap"
                    | "align-content"
                    | "align-items"
                    | "justify-content"
                    | "justify-items"
                    | "grid-auto-flow"
            )
    }

    fn set_property(&mut self, property: &str, value: &str) -> bool {
        if self.common.set_property(property, value) {
            return true;
        }
        match property {
            "gap" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.gap = Size {
                    width: value,
                    height: value,
                };
                true
            }),
            "column-gap" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.gap.width = value;
                true
            }),
            "row-gap" => parse_non_negative_length_percentage(value).is_some_and(|value| {
                self.gap.height = value;
                true
            }),
            "align-content" => value.parse().ok().is_some_and(|value| {
                self.align_content = Some(value);
                true
            }),
            "align-items" => value.parse().ok().is_some_and(|value| {
                self.align_items = Some(value);
                true
            }),
            "justify-content" => value.parse().ok().is_some_and(|value| {
                self.justify_content = Some(value);
                true
            }),
            "justify-items" => value.parse().ok().is_some_and(|value| {
                self.justify_items = Some(value);
                true
            }),
            "grid-auto-flow" => value.parse().ok().is_some_and(|value| {
                self.auto_flow = value;
                true
            }),
            _ => false,
        }
    }

    fn remove_property(&mut self, property: &str) -> bool {
        if self.common.remove_property(property) {
            return true;
        }

        let defaults = Self::default();

        match property {
            "gap" => self.gap = defaults.gap,
            "column-gap" => self.gap.width = defaults.gap.width,
            "row-gap" => self.gap.height = defaults.gap.height,
            "align-content" => self.align_content = defaults.align_content,
            "align-items" => self.align_items = defaults.align_items,
            "justify-content" => self.justify_content = defaults.justify_content,
            "justify-items" => self.justify_items = defaults.justify_items,
            "grid-auto-flow" => self.auto_flow = defaults.auto_flow,
            _ => return false,
        }
        true
    }
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            common: CommonStyle::default(),
            template_rows: Vec::new(),
            template_columns: Vec::new(),
            template_areas: Vec::new(),
            template_row_names: Vec::new(),
            template_column_names: Vec::new(),
            auto_rows: Vec::new(),
            auto_columns: Vec::new(),
            auto_flow: GridAutoFlow::Row,
            gap: Size {
                width: LengthPercentage::ZERO,
                height: LengthPercentage::ZERO,
            },
            align_content: None,
            justify_content: None,
            align_items: None,
            justify_items: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn style_is_send_and_sync() {
        assert_send_sync::<GridStyle>();
    }

    #[test]
    fn defaults_convert_to_taffy_grid_defaults() {
        let grid = GridStyle::default().to_taffy_style();
        let expected = taffy::Style::<String> {
            display: taffy::Display::Grid,
            ..Default::default()
        };

        assert_eq!(grid, expected);
    }

    #[test]
    fn conversion_preserves_grid_tracks() {
        let style = GridStyle {
            template_columns: vec![
                GridTemplateComponent::Single(TrackSizingFunction::length(100.0)),
                GridTemplateComponent::Single(TrackSizingFunction::fraction(1.0)),
            ],
            gap: Size {
                width: LengthPercentage::length(12.0),
                height: LengthPercentage::ZERO,
            },
            ..GridStyle::default()
        };
        let taffy = style.to_taffy_style();

        assert_eq!(taffy.grid_template_columns.len(), 2);
        assert_eq!(taffy.gap.width, taffy::LengthPercentage::length(12.0));
    }
}
