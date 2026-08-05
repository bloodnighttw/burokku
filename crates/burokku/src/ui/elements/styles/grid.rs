use taffy::{
    geometry::{Line, Size},
    AlignContent, AlignItems, AlignSelf, GridAutoFlow, GridPlacement, GridTemplateArea,
    GridTemplateComponent, JustifyContent, JustifyItems, JustifySelf, LengthPercentage,
    TrackSizingFunction,
};

use super::shared::{
    background::{impl_background_style, Background},
    border::{impl_border_style, Border},
    corner_radius::{impl_corner_radius_style, CornerRadius},
};

/// The properties used to lay out a grid container and its grid items.
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
    // Container properties
    pub template_rows: Vec<GridTemplateComponent<String>>,
    pub template_columns: Vec<GridTemplateComponent<String>>,
    pub template_areas: Vec<GridTemplateArea<String>>,
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
            template_rows: Vec::new(),
            template_columns: Vec::new(),
            template_areas: Vec::new(),
            template_row_names: Vec::new(),
            template_column_names: Vec::new(),
            auto_rows: Vec::new(),
            auto_columns: Vec::new(),
            auto_flow: GridAutoFlow::Row,
            gap: Size {
                width: LengthPercentage::length(0.0),
                height: LengthPercentage::length(0.0),
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

        assert_eq!(grid.template_rows, taffy.grid_template_rows);
        assert_eq!(grid.template_columns, taffy.grid_template_columns);
        assert_eq!(grid.template_areas, taffy.grid_template_areas);
        assert_eq!(grid.template_row_names, taffy.grid_template_row_names);
        assert_eq!(grid.template_column_names, taffy.grid_template_column_names);
        assert_eq!(grid.auto_rows, taffy.grid_auto_rows);
        assert_eq!(grid.auto_columns, taffy.grid_auto_columns);
        assert_eq!(grid.auto_flow, taffy.grid_auto_flow);
        assert_eq!(grid.gap, taffy.gap);
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
}
