use taffy::{
    geometry::{Line, MinMax, Size},
    AlignContent, AlignItems, AlignSelf, GridAutoFlow, JustifyContent, JustifyItems, JustifySelf,
};

use super::LengthPercentage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridTemplateArea {
    pub name: String,
    pub row_start: u16,
    pub row_end: u16,
    pub column_start: u16,
    pub column_end: u16,
}

impl GridTemplateArea {
    fn to_taffy(&self) -> taffy::GridTemplateArea<String> {
        taffy::GridTemplateArea {
            name: self.name.clone(),
            row_start: self.row_start,
            row_end: self.row_end,
            column_start: self.column_start,
            column_end: self.column_end,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GridPlacement {
    #[default]
    Auto,
    Line(i16),
    NamedLine(String, i16),
    Span(u16),
    NamedSpan(String, u16),
}

impl GridPlacement {
    fn to_taffy(&self) -> taffy::GridPlacement<String> {
        match self {
            Self::Auto => taffy::GridPlacement::Auto,
            Self::Line(index) => taffy::GridPlacement::Line((*index).into()),
            Self::NamedLine(name, index) => taffy::GridPlacement::NamedLine(name.clone(), *index),
            Self::Span(span) => taffy::GridPlacement::Span(*span),
            Self::NamedSpan(name, span) => taffy::GridPlacement::NamedSpan(name.clone(), *span),
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

impl MinTrackSizingFunction {
    fn to_taffy(self) -> taffy::MinTrackSizingFunction {
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

impl MaxTrackSizingFunction {
    fn to_taffy(self) -> taffy::MaxTrackSizingFunction {
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
            min: self.min.to_taffy(),
            max: self.max.to_taffy(),
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

impl GridTemplateRepetition {
    fn to_taffy(&self) -> taffy::GridTemplateRepetition<String> {
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

impl GridTemplateComponent {
    fn to_taffy(&self) -> taffy::GridTemplateComponent<String> {
        match self {
            Self::Single(track) => taffy::GridTemplateComponent::Single(track.to_taffy()),
            Self::Repeat(repetition) => taffy::GridTemplateComponent::Repeat(repetition.to_taffy()),
        }
    }
}

/// Thread-safe properties used to lay out a grid container and its grid items.
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
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

    // Item properties
    pub row: Line<GridPlacement>,
    pub column: Line<GridPlacement>,
    pub align_self: Option<AlignSelf>,
    pub justify_self: Option<JustifySelf>,
}

impl GridStyle {
    /// Build the Taffy value consumed only by MTS computed/layout state.
    pub fn to_taffy_style(&self) -> taffy::Style<String> {
        taffy::Style {
            display: taffy::Display::Grid,
            grid_template_rows: self
                .template_rows
                .iter()
                .map(GridTemplateComponent::to_taffy)
                .collect(),
            grid_template_columns: self
                .template_columns
                .iter()
                .map(GridTemplateComponent::to_taffy)
                .collect(),
            grid_template_areas: self
                .template_areas
                .iter()
                .map(GridTemplateArea::to_taffy)
                .collect(),
            grid_template_row_names: self.template_row_names.clone(),
            grid_template_column_names: self.template_column_names.clone(),
            grid_auto_rows: self
                .auto_rows
                .iter()
                .copied()
                .map(TrackSizingFunction::to_taffy)
                .collect(),
            grid_auto_columns: self
                .auto_columns
                .iter()
                .copied()
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
            grid_row: Line {
                start: self.row.start.to_taffy(),
                end: self.row.end.to_taffy(),
            },
            grid_column: Line {
                start: self.column.start.to_taffy(),
                end: self.column.end.to_taffy(),
            },
            align_self: self.align_self,
            justify_self: self.justify_self,
            ..taffy::Style::default()
        }
    }
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
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
            row: Line {
                start: GridPlacement::Auto,
                end: GridPlacement::Auto,
            },
            column: Line {
                start: GridPlacement::Auto,
                end: GridPlacement::Auto,
            },
            align_self: None,
            justify_self: None,
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
