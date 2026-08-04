use taffy::{
    geometry::{Point, Rect, Size},
    prelude::Display,
    style::{
        GridAutoTracks, GridPlacement, GridTemplateComponent, GridTemplateTracks,
        Position as TaffyPosition, Style as TaffyStyle,
    },
};

use crate::ui::elements::{
    styles::{Position, SizeValue, Style as ElementStyle},
    ElementKind,
};

pub(in crate::ui::layouts) fn to_taffy_style(
    kind: &ElementKind,
    style: &ElementStyle,
) -> TaffyStyle {
    let (grid_template_rows, grid_template_row_names) =
        grid_template(style.grid_template_rows.as_deref());
    let (grid_template_columns, grid_template_column_names) =
        grid_template(style.grid_template_columns.as_deref());
    let inset = if style.position == Position::Static {
        Rect {
            left: SizeValue::Auto.into(),
            right: SizeValue::Auto.into(),
            top: SizeValue::Auto.into(),
            bottom: SizeValue::Auto.into(),
        }
    } else {
        Rect {
            left: style.left.into(),
            right: style.right.into(),
            top: style.top.into(),
            bottom: style.bottom.into(),
        }
    };
    let position = match style.position {
        Position::Static | Position::Relative => TaffyPosition::Relative,
        Position::Absolute | Position::Fixed => TaffyPosition::Absolute,
    };
    TaffyStyle {
        display: if matches!(kind, ElementKind::Comment(_)) {
            Display::None
        } else {
            style.display
        },
        box_sizing: style.box_sizing,
        position,
        overflow: Point {
            x: style.overflow_x.into(),
            y: style.overflow_y.into(),
        },
        inset,
        size: Size {
            width: style.width.into(),
            height: style.height.into(),
        },
        min_size: Size {
            width: style.min_width.into(),
            height: style.min_height.into(),
        },
        max_size: Size {
            width: style.max_width.into(),
            height: style.max_height.into(),
        },
        aspect_ratio: style.aspect_ratio,
        margin: Rect {
            left: style.margin_left.into(),
            right: style.margin_right.into(),
            top: style.margin_top.into(),
            bottom: style.margin_bottom.into(),
        },
        padding: Rect {
            left: style.padding_left.into(),
            right: style.padding_right.into(),
            top: style.padding_top.into(),
            bottom: style.padding_bottom.into(),
        },
        border: Rect {
            left: style.border_left_width.into(),
            right: style.border_right_width.into(),
            top: style.border_top_width.into(),
            bottom: style.border_bottom_width.into(),
        },
        align_content: style.align_content,
        align_items: style.align_items,
        align_self: style.align_self,
        justify_content: style.justify_content,
        gap: Size {
            width: style.column_gap.into(),
            height: style.row_gap.into(),
        },
        flex_direction: style.flex_direction,
        flex_wrap: style.flex_wrap,
        flex_basis: style.flex_basis.into(),
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        grid_template_rows,
        grid_template_columns,
        grid_template_areas: style.grid_template_areas.clone(),
        grid_template_row_names,
        grid_template_column_names,
        grid_auto_rows: grid_auto_tracks(style.grid_auto_rows.as_deref()),
        grid_auto_columns: grid_auto_tracks(style.grid_auto_columns.as_deref()),
        grid_auto_flow: style.grid_auto_flow,
        grid_row: taffy::geometry::Line {
            start: grid_placement(style.grid_row_start.as_deref()),
            end: grid_placement(style.grid_row_end.as_deref()),
        },
        grid_column: taffy::geometry::Line {
            start: grid_placement(style.grid_column_start.as_deref()),
            end: grid_placement(style.grid_column_end.as_deref()),
        },
        ..TaffyStyle::default()
    }
}

type TemplateTracks = GridTemplateTracks<String, GridTemplateComponent<String>>;

fn grid_template(value: Option<&str>) -> (Vec<GridTemplateComponent<String>>, Vec<Vec<String>>) {
    value.map_or_else(
        || (Vec::new(), Vec::new()),
        |value| {
            let parsed = value
                .parse::<TemplateTracks>()
                .expect("grid templates are validated when styles are set");
            (parsed.tracks, parsed.line_names)
        },
    )
}

fn grid_auto_tracks(value: Option<&str>) -> Vec<taffy::style::TrackSizingFunction> {
    value.map_or_else(Vec::new, |value| {
        value
            .parse::<GridAutoTracks>()
            .expect("implicit grid tracks are validated when styles are set")
            .0
    })
}

fn grid_placement(value: Option<&str>) -> GridPlacement<String> {
    value.map_or(GridPlacement::Auto, |value| {
        value
            .parse()
            .expect("grid placement is validated when styles are set")
    })
}
