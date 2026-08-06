use taffy::{
    geometry::{Line, MinMax, Size},
    style::GridTemplateTracks,
    AlignContent, AlignItems, AlignSelf, CompactLength, GridAutoFlow, GridPlacement,
    GridTemplateComponent, GridTemplateRepetition, JustifyContent, JustifyItems, JustifySelf,
    MaxTrackSizingFunction, MinTrackSizingFunction, RepetitionCount, TrackSizingFunction,
};

use super::shared::{
    background::{impl_background_style, Background},
    border::{impl_border_style, Border},
    corner_radius::{impl_corner_radius_style, CornerRadius},
};

/// A parsed, thread-safe grid template stored directly in [`super::super::Elements`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridTemplate {
    pub tracks: Vec<GridTemplateTrack>,
    pub line_names: Vec<Vec<String>>,
}

/// One component of a parsed grid template.
#[derive(Clone, Debug, PartialEq)]
pub enum GridTemplateTrack {
    Single(GridTrackSizing),
    Repeat(GridTrackRepetition),
}

/// A parsed `repeat(...)` grid component.
#[derive(Clone, Debug, PartialEq)]
pub struct GridTrackRepetition {
    pub count: RepetitionCount,
    pub tracks: Vec<GridTrackSizing>,
    pub line_names: Vec<Vec<String>>,
}

/// Parsed minimum and maximum sizing functions for one grid track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridTrackSizing {
    pub min: GridMinTrackSizing,
    pub max: GridMaxTrackSizing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridMinTrackSizing {
    Length(f32),
    Percent(f32),
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridMaxTrackSizing {
    Length(f32),
    Percent(f32),
    Auto,
    MinContent,
    MaxContent,
    Fraction(f32),
    FitContentLength(f32),
    FitContentPercent(f32),
}

impl GridTemplate {
    pub(crate) fn from_taffy(
        template: GridTemplateTracks<String, GridTemplateComponent<String>>,
    ) -> Self {
        Self {
            tracks: template
                .tracks
                .into_iter()
                .map(GridTemplateTrack::from_taffy)
                .collect(),
            line_names: template.line_names,
        }
    }

    pub(crate) fn to_taffy(&self) -> GridTemplateTracks<String, GridTemplateComponent<String>> {
        GridTemplateTracks {
            tracks: self
                .tracks
                .iter()
                .map(GridTemplateTrack::to_taffy)
                .collect(),
            line_names: self.line_names.clone(),
        }
    }
}

impl GridTemplateTrack {
    fn from_taffy(track: GridTemplateComponent<String>) -> Self {
        match track {
            GridTemplateComponent::Single(track) => {
                Self::Single(GridTrackSizing::from_taffy(track))
            }
            GridTemplateComponent::Repeat(repetition) => Self::Repeat(GridTrackRepetition {
                count: repetition.count,
                tracks: repetition
                    .tracks
                    .into_iter()
                    .map(GridTrackSizing::from_taffy)
                    .collect(),
                line_names: repetition.line_names,
            }),
        }
    }

    fn to_taffy(&self) -> GridTemplateComponent<String> {
        match self {
            Self::Single(track) => GridTemplateComponent::Single(track.to_taffy()),
            Self::Repeat(repetition) => GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: repetition.count,
                tracks: repetition
                    .tracks
                    .iter()
                    .map(GridTrackSizing::to_taffy)
                    .collect(),
                line_names: repetition.line_names.clone(),
            }),
        }
    }
}

impl GridTrackSizing {
    pub(crate) fn from_taffy(track: TrackSizingFunction) -> Self {
        Self {
            min: GridMinTrackSizing::from_taffy(track.min_sizing_function()),
            max: GridMaxTrackSizing::from_taffy(track.max_sizing_function()),
        }
    }

    pub(crate) fn to_taffy(&self) -> TrackSizingFunction {
        MinMax {
            min: self.min.to_taffy(),
            max: self.max.to_taffy(),
        }
    }
}

impl GridMinTrackSizing {
    fn from_taffy(value: MinTrackSizingFunction) -> Self {
        let value = value.into_raw();
        match value.tag() {
            CompactLength::LENGTH_TAG => Self::Length(value.value()),
            CompactLength::PERCENT_TAG => Self::Percent(value.value()),
            CompactLength::AUTO_TAG => Self::Auto,
            CompactLength::MIN_CONTENT_TAG => Self::MinContent,
            CompactLength::MAX_CONTENT_TAG => Self::MaxContent,
            tag => unreachable!("unsupported parsed minimum grid-track tag {tag}"),
        }
    }

    fn to_taffy(self) -> MinTrackSizingFunction {
        match self {
            Self::Length(value) => MinTrackSizingFunction::length(value),
            Self::Percent(value) => MinTrackSizingFunction::percent(value),
            Self::Auto => MinTrackSizingFunction::auto(),
            Self::MinContent => MinTrackSizingFunction::min_content(),
            Self::MaxContent => MinTrackSizingFunction::max_content(),
        }
    }
}

impl GridMaxTrackSizing {
    fn from_taffy(value: MaxTrackSizingFunction) -> Self {
        let value = value.into_raw();
        match value.tag() {
            CompactLength::LENGTH_TAG => Self::Length(value.value()),
            CompactLength::PERCENT_TAG => Self::Percent(value.value()),
            CompactLength::AUTO_TAG => Self::Auto,
            CompactLength::MIN_CONTENT_TAG => Self::MinContent,
            CompactLength::MAX_CONTENT_TAG => Self::MaxContent,
            CompactLength::FR_TAG => Self::Fraction(value.value()),
            CompactLength::FIT_CONTENT_PX_TAG => Self::FitContentLength(value.value()),
            CompactLength::FIT_CONTENT_PERCENT_TAG => Self::FitContentPercent(value.value()),
            tag => unreachable!("unsupported parsed maximum grid-track tag {tag}"),
        }
    }

    fn to_taffy(self) -> MaxTrackSizingFunction {
        match self {
            Self::Length(value) => MaxTrackSizingFunction::length(value),
            Self::Percent(value) => MaxTrackSizingFunction::percent(value),
            Self::Auto => MaxTrackSizingFunction::auto(),
            Self::MinContent => MaxTrackSizingFunction::min_content(),
            Self::MaxContent => MaxTrackSizingFunction::max_content(),
            Self::Fraction(value) => MaxTrackSizingFunction::fr(value),
            Self::FitContentLength(value) => MaxTrackSizingFunction::fit_content_px(value),
            Self::FitContentPercent(value) => MaxTrackSizingFunction::fit_content_percent(value),
        }
    }
}

/// The properties used to lay out a grid container and its grid items.
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
    // Container properties
    pub template_rows: GridTemplate,
    pub template_columns: GridTemplate,
    pub auto_rows: Vec<GridTrackSizing>,
    pub auto_columns: Vec<GridTrackSizing>,
    pub auto_flow: GridAutoFlow,
    pub gap: Size<f32>,
    pub align_content: Option<AlignContent>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub justify_items: Option<JustifyItems>,

    // Item properties
    pub row: Line<GridPlacement<String>>,
    pub column: Line<GridPlacement<String>>,
    pub align_self: Option<AlignSelf>,
    pub justify_self: Option<JustifySelf>,

    // Shared paint properties
    pub background: Background,
    pub border: Option<Border>,
    pub corner_radius: CornerRadius,
}

impl_background_style!(GridStyle);
impl_border_style!(GridStyle);
impl_corner_radius_style!(GridStyle);

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            template_rows: GridTemplate::default(),
            template_columns: GridTemplate::default(),
            auto_rows: Vec::new(),
            auto_columns: Vec::new(),
            auto_flow: GridAutoFlow::Row,
            gap: Size {
                width: 0.0,
                height: 0.0,
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
            background: Background::default(),
            border: None,
            corner_radius: CornerRadius::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_taffy_grid_defaults() {
        let grid = GridStyle::default();
        let taffy = taffy::Style::<String>::default();

        assert_eq!(grid.template_rows, GridTemplate::default());
        assert_eq!(grid.template_columns, GridTemplate::default());
        assert!(grid.auto_rows.is_empty());
        assert!(grid.auto_columns.is_empty());
        assert_eq!(grid.auto_flow, taffy.grid_auto_flow);
        assert_eq!(grid.gap.width, 0.0);
        assert_eq!(grid.gap.height, 0.0);
        assert_eq!(grid.align_content, taffy.align_content);
        assert_eq!(grid.justify_content, taffy.justify_content);
        assert_eq!(grid.align_items, taffy.align_items);
        assert_eq!(grid.justify_items, taffy.justify_items);
        assert_eq!(grid.row, taffy.grid_row);
        assert_eq!(grid.column, taffy.grid_column);
        assert_eq!(grid.align_self, taffy.align_self);
        assert_eq!(grid.justify_self, taffy.justify_self);
        assert_eq!(grid.background, Background::default());
        assert_eq!(grid.border, None);
        assert_eq!(grid.corner_radius, CornerRadius::ZERO);
    }

    #[test]
    fn parsed_track_data_round_trips_without_retaining_css_source() {
        type TaffyTemplate = GridTemplateTracks<String, GridTemplateComponent<String>>;

        for source in [
            "20px 1fr",
            "minmax(10px, 2fr) 25% fit-content(50px)",
            "[start] repeat(2, [inner] 12px 1fr) [end]",
            "repeat(auto-fill, 24px)",
        ] {
            let parsed = source.parse::<TaffyTemplate>().unwrap();
            let native = GridTemplate::from_taffy(parsed.clone());

            assert_eq!(native.to_taffy(), parsed, "{source}");
        }
    }
}
