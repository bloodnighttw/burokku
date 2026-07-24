use std::borrow::Cow;

use super::{
    AlignContent, AlignItems, BorderStyle, BoxSizing, Color, CornerRadiusValue, Display,
    FlexDirection, FlexWrap, Isolation, LengthPercentageValue, LengthValue, LineHeightValue,
    MaxSizeValue, Overflow, Position, SizeValue, Style, ZIndex,
};
use taffy::style::{
    GridAutoTracks, GridPlacement, GridTemplateArea, GridTemplateComponent, GridTemplateTracks,
    TrackSizingFunction,
};
use thiserror::Error;

pub(crate) fn set_style(
    style: &mut Style,
    name: &str,
    value: Option<&str>,
) -> Result<(), StyleError> {
    let name = normalized_name(name);
    let name = name.as_ref();
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return clear_style(style, name);
    };

    macro_rules! size {
        ($field:ident, $negative:expr) => {{
            style.$field = parse_size(name, value, $negative)?;
        }};
    }
    macro_rules! length_percentage {
        ($field:ident, $negative:expr) => {{
            style.$field = parse_length_percentage(name, value, $negative)?;
        }};
    }
    macro_rules! length {
        ($field:ident, $negative:expr) => {{
            style.$field = parse_length_value(name, value, $negative)?;
        }};
    }
    macro_rules! color {
        ($field:ident) => {{
            style.$field = Some(parse_color(name, value)?);
        }};
    }

    match name {
        "display" => {
            style.display = match value {
                "block" => Display::Block,
                "flex" | "inline-flex" => Display::Flex,
                "grid" | "inline-grid" => Display::Grid,
                "none" => Display::None,
                _ => return invalid(name, value),
            };
        }
        "box-sizing" => {
            style.box_sizing = match value {
                "border-box" => BoxSizing::BorderBox,
                "content-box" => BoxSizing::ContentBox,
                _ => return invalid(name, value),
            };
        }
        "position" => {
            style.position = match value {
                "static" => Position::Static,
                "relative" => Position::Relative,
                "absolute" => Position::Absolute,
                "fixed" => Position::Fixed,
                _ => return invalid(name, value),
            };
        }
        "overflow" => {
            let values = one_or_two(name, value, parse_overflow)?;
            style.overflow_x = values.0;
            style.overflow_y = values.1;
        }
        "overflow-x" => style.overflow_x = parse_overflow(name, value)?,
        "overflow-y" => style.overflow_y = parse_overflow(name, value)?,
        "z-index" => {
            style.z_index = if value == "auto" {
                ZIndex::Auto
            } else {
                ZIndex::Value(
                    value
                        .parse()
                        .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))?,
                )
            };
        }
        "isolation" => {
            style.isolation = match value {
                "auto" => Isolation::Auto,
                "isolate" => Isolation::Isolate,
                _ => return invalid(name, value),
            };
        }

        "width" => size!(width, false),
        "height" => size!(height, false),
        "min-width" => size!(min_width, false),
        "min-height" => size!(min_height, false),
        "max-width" => style.max_width = parse_max_size(name, value)?,
        "max-height" => style.max_height = parse_max_size(name, value)?,
        "aspect-ratio" => style.aspect_ratio = Some(parse_aspect_ratio(name, value)?),
        "top" => size!(top, true),
        "right" => size!(right, true),
        "bottom" => size!(bottom, true),
        "left" => size!(left, true),
        "inset" => {
            let [top, right, bottom, left] = parse_box_sizes(name, value, true)?;
            style.top = top;
            style.right = right;
            style.bottom = bottom;
            style.left = left;
        }

        "margin" => {
            let [top, right, bottom, left] = parse_box_sizes(name, value, true)?;
            style.margin_top = top;
            style.margin_right = right;
            style.margin_bottom = bottom;
            style.margin_left = left;
        }
        "margin-top" => size!(margin_top, true),
        "margin-right" => size!(margin_right, true),
        "margin-bottom" => size!(margin_bottom, true),
        "margin-left" => size!(margin_left, true),
        "padding" => {
            let [top, right, bottom, left] = parse_box_length_percentages(name, value, false)?;
            style.padding_top = top;
            style.padding_right = right;
            style.padding_bottom = bottom;
            style.padding_left = left;
        }
        "padding-top" => length_percentage!(padding_top, false),
        "padding-right" => length_percentage!(padding_right, false),
        "padding-bottom" => length_percentage!(padding_bottom, false),
        "padding-left" => length_percentage!(padding_left, false),
        "border-width" => {
            let [top, right, bottom, left] = parse_box_lengths(name, value, false)?;
            style.border_top_width = top;
            style.border_right_width = right;
            style.border_bottom_width = bottom;
            style.border_left_width = left;
        }
        "border-top-width" => length!(border_top_width, false),
        "border-right-width" => length!(border_right_width, false),
        "border-bottom-width" => length!(border_bottom_width, false),
        "border-left-width" => length!(border_left_width, false),

        "gap" => {
            let (row, column) = one_or_two(name, value, |name, value| {
                parse_length_percentage(name, value, false)
            })?;
            style.row_gap = row;
            style.column_gap = column;
        }
        "row-gap" => length_percentage!(row_gap, false),
        "column-gap" => length_percentage!(column_gap, false),
        "flex-direction" => {
            style.flex_direction = match value {
                "row" => FlexDirection::Row,
                "row-reverse" => FlexDirection::RowReverse,
                "column" => FlexDirection::Column,
                "column-reverse" => FlexDirection::ColumnReverse,
                _ => return invalid(name, value),
            };
        }
        "flex-wrap" => {
            style.flex_wrap = match value {
                "nowrap" => FlexWrap::NoWrap,
                "wrap" => FlexWrap::Wrap,
                "wrap-reverse" => FlexWrap::WrapReverse,
                _ => return invalid(name, value),
            };
        }
        "flex-basis" => size!(flex_basis, false),
        "flex-grow" => style.flex_grow = parse_non_negative_number(name, value)?,
        "flex-shrink" => style.flex_shrink = parse_non_negative_number(name, value)?,
        "flex" => {
            let (grow, shrink, basis) = parse_flex(name, value)?;
            style.flex_grow = grow;
            style.flex_shrink = shrink;
            style.flex_basis = basis;
        }
        "order" => {
            style.order = value
                .parse()
                .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))?
        }
        "align-items" => style.align_items = Some(parse_align_items(name, value)?),
        "align-self" => {
            style.align_self = if value == "auto" {
                None
            } else {
                Some(parse_align_items(name, value)?)
            };
        }
        "align-content" => style.align_content = Some(parse_align_content(name, value)?),
        "justify-content" => style.justify_content = Some(parse_align_content(name, value)?),

        "grid-template-rows" => set_grid_template_rows(style, name, value)?,
        "grid-template-columns" => set_grid_template_columns(style, name, value)?,
        "grid-template-areas" => {
            style.grid_template_areas = parse_grid_template_areas(name, value)?
        }
        "grid-template" => {
            if value == "none" {
                style.grid_template_rows = None;
                style.grid_template_columns = None;
                style.grid_template_areas.clear();
                return Ok(());
            }
            let (rows, columns) = split_grid_shorthand(name, value)?;
            let columns = parse_grid_template(name, columns)?;
            if rows.starts_with(['"', '\'']) {
                let (rows, areas) = parse_grid_area_template_rows(name, rows)?;
                style.grid_template_rows = Some(rows);
                style.grid_template_areas = areas;
            } else {
                style.grid_template_rows = parse_grid_template(name, rows)?;
                style.grid_template_areas.clear();
            }
            style.grid_template_columns = columns;
        }
        "grid-auto-rows" => {
            parse_css::<GridAutoTracks>(name, value)?;
            style.grid_auto_rows = Some(value.into());
        }
        "grid-auto-columns" => {
            parse_css::<GridAutoTracks>(name, value)?;
            style.grid_auto_columns = Some(value.into());
        }
        "grid-auto-flow" => style.grid_auto_flow = parse_css(name, value)?,
        "grid-row-start" => style.grid_row_start = parse_grid_placement(name, value)?,
        "grid-row-end" => style.grid_row_end = parse_grid_placement(name, value)?,
        "grid-column-start" => style.grid_column_start = parse_grid_placement(name, value)?,
        "grid-column-end" => style.grid_column_end = parse_grid_placement(name, value)?,
        "grid-row" => {
            let (start, end) = parse_grid_axis(name, value)?;
            style.grid_row_start = start;
            style.grid_row_end = end;
        }
        "grid-column" => {
            let (start, end) = parse_grid_axis(name, value)?;
            style.grid_column_start = start;
            style.grid_column_end = end;
        }
        "grid-area" => {
            let [row_start, column_start, row_end, column_end] = parse_grid_area(name, value)?;
            style.grid_row_start = row_start;
            style.grid_column_start = column_start;
            style.grid_row_end = row_end;
            style.grid_column_end = column_end;
        }

        "background-color" => color!(background_color),
        "color" => color!(color),
        "border-color" => {
            let [top, right, bottom, left] = parse_box_colors(name, value)?;
            style.border_top_color = Some(top);
            style.border_right_color = Some(right);
            style.border_bottom_color = Some(bottom);
            style.border_left_color = Some(left);
        }
        "border-top-color" => color!(border_top_color),
        "border-right-color" => color!(border_right_color),
        "border-bottom-color" => color!(border_bottom_color),
        "border-left-color" => color!(border_left_color),
        "border-style" => {
            let [top, right, bottom, left] =
                parse_box_values(name, value, |part| parse_border_style(name, part))?;
            style.border_top_style = top;
            style.border_right_style = right;
            style.border_bottom_style = bottom;
            style.border_left_style = left;
        }
        "border-top-style" => style.border_top_style = parse_border_style(name, value)?,
        "border-right-style" => style.border_right_style = parse_border_style(name, value)?,
        "border-bottom-style" => style.border_bottom_style = parse_border_style(name, value)?,
        "border-left-style" => style.border_left_style = parse_border_style(name, value)?,
        "border-radius" => {
            let [top_left, top_right, bottom_right, bottom_left] =
                parse_border_radius(name, value)?;
            style.border_top_left_radius = top_left;
            style.border_top_right_radius = top_right;
            style.border_bottom_right_radius = bottom_right;
            style.border_bottom_left_radius = bottom_left;
        }
        "border-top-left-radius" => {
            style.border_top_left_radius = parse_corner_radius(name, value)?
        }
        "border-top-right-radius" => {
            style.border_top_right_radius = parse_corner_radius(name, value)?
        }
        "border-bottom-right-radius" => {
            style.border_bottom_right_radius = parse_corner_radius(name, value)?
        }
        "border-bottom-left-radius" => {
            style.border_bottom_left_radius = parse_corner_radius(name, value)?
        }
        "outline-color" => color!(outline_color),
        "outline-width" => length!(outline_width, false),
        "outline-offset" => length!(outline_offset, true),

        "font-size" => style.font_size = Some(parse_length_percentage(name, value, false)?),
        "line-height" => style.line_height = Some(parse_line_height(name, value)?),
        "font-weight" => {
            style.font_weight = Some(match value {
                "normal" => 400,
                "bold" => 700,
                _ => value
                    .parse::<u16>()
                    .ok()
                    .filter(|weight| (1..=1000).contains(weight))
                    .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?,
            });
        }
        "font-family" => style.font_family = Some(value.trim_matches(['\'', '"']).into()),
        _ => return Err(StyleError::UnsupportedProperty(name.into())),
    }

    Ok(())
}

fn clear_style(style: &mut Style, name: &str) -> Result<(), StyleError> {
    let default = Style::default();
    macro_rules! reset {
        ($field:ident) => {{
            style.$field = default.$field;
        }};
    }
    macro_rules! reset_box {
        ($top:ident, $right:ident, $bottom:ident, $left:ident) => {{
            reset!($top);
            reset!($right);
            reset!($bottom);
            reset!($left);
        }};
    }

    match name {
        "display" => reset!(display),
        "box-sizing" => reset!(box_sizing),
        "position" => reset!(position),
        "overflow" => {
            reset!(overflow_x);
            reset!(overflow_y);
        }
        "overflow-x" => reset!(overflow_x),
        "overflow-y" => reset!(overflow_y),
        "z-index" => reset!(z_index),
        "isolation" => reset!(isolation),
        "width" => reset!(width),
        "height" => reset!(height),
        "min-width" => reset!(min_width),
        "min-height" => reset!(min_height),
        "max-width" => reset!(max_width),
        "max-height" => reset!(max_height),
        "aspect-ratio" => reset!(aspect_ratio),
        "inset" => reset_box!(top, right, bottom, left),
        "top" => reset!(top),
        "right" => reset!(right),
        "bottom" => reset!(bottom),
        "left" => reset!(left),
        "margin" => reset_box!(margin_top, margin_right, margin_bottom, margin_left),
        "margin-top" => reset!(margin_top),
        "margin-right" => reset!(margin_right),
        "margin-bottom" => reset!(margin_bottom),
        "margin-left" => reset!(margin_left),
        "padding" => reset_box!(padding_top, padding_right, padding_bottom, padding_left),
        "padding-top" => reset!(padding_top),
        "padding-right" => reset!(padding_right),
        "padding-bottom" => reset!(padding_bottom),
        "padding-left" => reset!(padding_left),
        "border-width" => reset_box!(
            border_top_width,
            border_right_width,
            border_bottom_width,
            border_left_width
        ),
        "border-top-width" => reset!(border_top_width),
        "border-right-width" => reset!(border_right_width),
        "border-bottom-width" => reset!(border_bottom_width),
        "border-left-width" => reset!(border_left_width),
        "gap" => {
            reset!(row_gap);
            reset!(column_gap);
        }
        "row-gap" => reset!(row_gap),
        "column-gap" => reset!(column_gap),
        "flex-direction" => reset!(flex_direction),
        "flex-wrap" => reset!(flex_wrap),
        "flex-basis" => reset!(flex_basis),
        "flex-grow" => reset!(flex_grow),
        "flex-shrink" => reset!(flex_shrink),
        "flex" => {
            reset!(flex_grow);
            reset!(flex_shrink);
            reset!(flex_basis);
        }
        "order" => reset!(order),
        "align-items" => reset!(align_items),
        "align-self" => reset!(align_self),
        "align-content" => reset!(align_content),
        "justify-content" => reset!(justify_content),
        "grid-template-rows" => {
            reset!(grid_template_rows);
        }
        "grid-template-columns" => {
            reset!(grid_template_columns);
        }
        "grid-template-areas" => reset!(grid_template_areas),
        "grid-template" => {
            reset!(grid_template_rows);
            reset!(grid_template_columns);
            reset!(grid_template_areas);
        }
        "grid-auto-rows" => reset!(grid_auto_rows),
        "grid-auto-columns" => reset!(grid_auto_columns),
        "grid-auto-flow" => reset!(grid_auto_flow),
        "grid-row-start" => reset!(grid_row_start),
        "grid-row-end" => reset!(grid_row_end),
        "grid-column-start" => reset!(grid_column_start),
        "grid-column-end" => reset!(grid_column_end),
        "grid-row" => {
            reset!(grid_row_start);
            reset!(grid_row_end);
        }
        "grid-column" => {
            reset!(grid_column_start);
            reset!(grid_column_end);
        }
        "grid-area" => {
            reset!(grid_row_start);
            reset!(grid_row_end);
            reset!(grid_column_start);
            reset!(grid_column_end);
        }
        "background-color" => reset!(background_color),
        "color" => reset!(color),
        "border-color" => reset_box!(
            border_top_color,
            border_right_color,
            border_bottom_color,
            border_left_color
        ),
        "border-top-color" => reset!(border_top_color),
        "border-right-color" => reset!(border_right_color),
        "border-bottom-color" => reset!(border_bottom_color),
        "border-left-color" => reset!(border_left_color),
        "border-style" => reset_box!(
            border_top_style,
            border_right_style,
            border_bottom_style,
            border_left_style
        ),
        "border-top-style" => reset!(border_top_style),
        "border-right-style" => reset!(border_right_style),
        "border-bottom-style" => reset!(border_bottom_style),
        "border-left-style" => reset!(border_left_style),
        "border-radius" => reset_box!(
            border_top_left_radius,
            border_top_right_radius,
            border_bottom_right_radius,
            border_bottom_left_radius
        ),
        "border-top-left-radius" => reset!(border_top_left_radius),
        "border-top-right-radius" => reset!(border_top_right_radius),
        "border-bottom-right-radius" => reset!(border_bottom_right_radius),
        "border-bottom-left-radius" => reset!(border_bottom_left_radius),
        "outline-color" => reset!(outline_color),
        "outline-width" => reset!(outline_width),
        "outline-offset" => reset!(outline_offset),
        "font-size" => reset!(font_size),
        "line-height" => reset!(line_height),
        "font-weight" => reset!(font_weight),
        "font-family" => reset!(font_family),
        _ => return Err(StyleError::UnsupportedProperty(name.into())),
    }

    Ok(())
}

fn normalized_name(name: &str) -> Cow<'_, str> {
    if !name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Cow::Borrowed(name);
    }

    let mut normalized = String::with_capacity(name.len() + 4);
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            normalized.push('-');
            normalized.push(character.to_ascii_lowercase());
        } else {
            normalized.push(character);
        }
    }
    Cow::Owned(normalized)
}

fn parse_flex(name: &str, value: &str) -> Result<(f32, f32, SizeValue), StyleError> {
    match value {
        "none" => return Ok((0.0, 0.0, SizeValue::Auto)),
        "auto" => return Ok((1.0, 1.0, SizeValue::Auto)),
        "initial" => return Ok((0.0, 1.0, SizeValue::Auto)),
        _ => {}
    }

    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    let parse_factor = |part: &str| parse_non_negative_number(name, part);
    match parts.as_slice() {
        [one] => {
            if let Ok(grow) = parse_factor(one) {
                Ok((grow, 1.0, SizeValue::Percent(0.0)))
            } else {
                Ok((1.0, 1.0, parse_size(name, one, false)?))
            }
        }
        [first, second] => {
            let grow = parse_factor(first)?;
            if let Ok(shrink) = parse_factor(second) {
                Ok((grow, shrink, SizeValue::Percent(0.0)))
            } else {
                Ok((grow, 1.0, parse_size(name, second, false)?))
            }
        }
        [first, second, third] => Ok((
            parse_factor(first)?,
            parse_factor(second)?,
            parse_size(name, third, false)?,
        )),
        _ => invalid(name, value),
    }
}

fn set_grid_template_rows(style: &mut Style, name: &str, value: &str) -> Result<(), StyleError> {
    style.grid_template_rows = parse_grid_template(name, value)?;
    Ok(())
}

fn set_grid_template_columns(style: &mut Style, name: &str, value: &str) -> Result<(), StyleError> {
    style.grid_template_columns = parse_grid_template(name, value)?;
    Ok(())
}

type TemplateTracks = GridTemplateTracks<String, GridTemplateComponent<String>>;

fn parse_grid_template(name: &str, value: &str) -> Result<Option<String>, StyleError> {
    if value == "none" {
        return Ok(None);
    }
    parse_css::<TemplateTracks>(name, value)?;
    Ok(Some(value.into()))
}

fn split_grid_shorthand<'a>(name: &str, value: &'a str) -> Result<(&'a str, &'a str), StyleError> {
    let mut depth = 0_u32;
    let mut slash = None;
    for (index, character) in value.char_indices() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?
            }
            '/' if depth == 0 && slash.replace(index).is_some() => return invalid(name, value),
            _ => {}
        }
    }
    if depth != 0 {
        return invalid(name, value);
    }
    let Some(slash) = slash else {
        return invalid(name, value);
    };
    let rows = value[..slash].trim();
    let columns = value[slash + 1..].trim();
    if rows.is_empty() || columns.is_empty() {
        return invalid(name, value);
    }
    Ok((rows, columns))
}

fn parse_grid_template_areas(
    name: &str,
    value: &str,
) -> Result<Vec<GridTemplateArea<String>>, StyleError> {
    if value == "none" {
        return Ok(Vec::new());
    }

    build_grid_template_areas(name, value, parse_quoted_grid_rows(name, value)?)
}

fn build_grid_template_areas(
    name: &str,
    value: &str,
    rows: Vec<Vec<String>>,
) -> Result<Vec<GridTemplateArea<String>>, StyleError> {
    let column_count = rows.first().map_or(0, Vec::len);
    if column_count == 0
        || rows.iter().any(|row| row.len() != column_count)
        || rows.len() >= u16::MAX as usize
        || column_count >= u16::MAX as usize
    {
        return invalid(name, value);
    }

    let mut names = Vec::new();
    for cell in rows.iter().flatten() {
        if cell.chars().all(|character| character == '.') {
            continue;
        }
        if cell.contains('.') || !valid_grid_area_name(cell) {
            return invalid(name, value);
        }
        if !names.contains(cell) {
            names.push(cell.clone());
        }
    }

    let mut areas = Vec::with_capacity(names.len());
    for area_name in names {
        let mut row_start = usize::MAX;
        let mut row_end = 0;
        let mut column_start = usize::MAX;
        let mut column_end = 0;
        for (row, cells) in rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                if cell == &area_name {
                    row_start = row_start.min(row);
                    row_end = row_end.max(row + 1);
                    column_start = column_start.min(column);
                    column_end = column_end.max(column + 1);
                }
            }
        }
        if rows[row_start..row_end].iter().any(|row| {
            row[column_start..column_end]
                .iter()
                .any(|cell| cell != &area_name)
        }) {
            return invalid(name, value);
        }
        areas.push(GridTemplateArea {
            name: area_name,
            row_start: row_start as u16 + 1,
            row_end: row_end as u16 + 1,
            column_start: column_start as u16 + 1,
            column_end: column_end as u16 + 1,
        });
    }
    Ok(areas)
}

fn parse_grid_area_template_rows(
    name: &str,
    value: &str,
) -> Result<(String, Vec<GridTemplateArea<String>>), StyleError> {
    let mut rows = Vec::new();
    let mut track_sizes = Vec::new();
    let mut cursor = 0;
    while cursor < value.len() {
        cursor = skip_ascii_whitespace(value, cursor);
        if cursor == value.len() {
            break;
        }
        let quote = value.as_bytes()[cursor];
        if quote != b'"' && quote != b'\'' {
            return invalid(name, value);
        }
        let row_start = cursor + 1;
        cursor = row_start;
        while cursor < value.len() && value.as_bytes()[cursor] != quote {
            if value.as_bytes()[cursor] == b'\\' {
                return invalid(name, value);
            }
            cursor += 1;
        }
        if cursor == value.len() {
            return invalid(name, value);
        }
        let row = value[row_start..cursor]
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if row.is_empty() {
            return invalid(name, value);
        }
        rows.push(row);
        cursor += 1;
        cursor = skip_ascii_whitespace(value, cursor);

        if cursor == value.len() || matches!(value.as_bytes()[cursor], b'"' | b'\'') {
            track_sizes.push("auto".to_owned());
            continue;
        }
        let track_start = cursor;
        let mut depth = 0_u32;
        while cursor < value.len() {
            match value.as_bytes()[cursor] {
                b'(' => depth += 1,
                b')' => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?
                }
                byte if byte.is_ascii_whitespace() && depth == 0 => break,
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            return invalid(name, value);
        }
        let track = &value[track_start..cursor];
        parse_css::<TrackSizingFunction>(name, track)?;
        track_sizes.push(track.to_owned());
    }
    if rows.is_empty() {
        return invalid(name, value);
    }
    let areas = build_grid_template_areas(name, value, rows)?;
    Ok((track_sizes.join(" "), areas))
}

fn skip_ascii_whitespace(value: &str, mut cursor: usize) -> usize {
    while cursor < value.len() && value.as_bytes()[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn parse_quoted_grid_rows(name: &str, value: &str) -> Result<Vec<Vec<String>>, StyleError> {
    let mut rows = Vec::new();
    let mut characters = value.char_indices().peekable();
    while let Some((_, character)) = characters.next() {
        if character.is_ascii_whitespace() {
            continue;
        }
        if character != '"' && character != '\'' {
            return invalid(name, value);
        }
        let quote = character;
        let start = characters.peek().map_or(value.len(), |(index, _)| *index);
        let mut end = None;
        for (index, character) in characters.by_ref() {
            if character == '\\' {
                return invalid(name, value);
            }
            if character == quote {
                end = Some(index);
                break;
            }
        }
        let Some(end) = end else {
            return invalid(name, value);
        };
        let row = value[start..end]
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if row.is_empty() {
            return invalid(name, value);
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return invalid(name, value);
    }
    Ok(rows)
}

fn valid_grid_area_name(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(
        characters.next(),
        Some(character) if character.is_ascii_alphabetic() || character == '_' || character == '-'
    ) && characters
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

fn parse_grid_placement(name: &str, value: &str) -> Result<Option<String>, StyleError> {
    parse_css::<GridPlacement<String>>(name, value)?;
    Ok((value != "auto").then(|| value.into()))
}

fn parse_grid_axis(
    name: &str,
    value: &str,
) -> Result<(Option<String>, Option<String>), StyleError> {
    let parts = value.split('/').map(str::trim).collect::<Vec<_>>();
    match parts.as_slice() {
        [start] if !start.is_empty() => Ok((parse_grid_placement(name, start)?, None)),
        [start, end] if !start.is_empty() && !end.is_empty() => Ok((
            parse_grid_placement(name, start)?,
            parse_grid_placement(name, end)?,
        )),
        _ => invalid(name, value),
    }
}

fn parse_grid_area(name: &str, value: &str) -> Result<[Option<String>; 4], StyleError> {
    let parts = value.split('/').map(str::trim).collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
        return invalid(name, value);
    }
    let row_start = parse_grid_placement(name, parts[0])?;
    let column_start = if let Some(value) = parts.get(1) {
        parse_grid_placement(name, value)?
    } else {
        repeated_named_placement(&row_start)
    };
    let row_end = if let Some(value) = parts.get(2) {
        parse_grid_placement(name, value)?
    } else {
        repeated_named_placement(&row_start)
    };
    let column_end = if let Some(value) = parts.get(3) {
        parse_grid_placement(name, value)?
    } else {
        repeated_named_placement(&column_start)
    };
    Ok([row_start, column_start, row_end, column_end])
}

fn repeated_named_placement(value: &Option<String>) -> Option<String> {
    let value = value.as_deref()?;
    matches!(
        parse_css::<GridPlacement<String>>("grid-area", value).ok()?,
        GridPlacement::NamedLine(_, _)
    )
    .then(|| value.into())
}

fn parse_css<T>(name: &str, value: &str) -> Result<T, StyleError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))
}

fn parse_size(name: &str, value: &str, allow_negative: bool) -> Result<SizeValue, StyleError> {
    if value == "auto" {
        return Ok(SizeValue::Auto);
    }
    if let Some(percent) = value.strip_suffix('%') {
        let percent = parse_number(name, percent.trim())?;
        if !allow_negative && percent < 0.0 {
            return invalid(name, value);
        }
        return Ok(SizeValue::percent(percent));
    }
    let pixels = parse_length(name, value)?;
    if !allow_negative && pixels < 0.0 {
        return invalid(name, value);
    }
    Ok(SizeValue::px(pixels))
}

fn parse_max_size(name: &str, value: &str) -> Result<MaxSizeValue, StyleError> {
    if value == "none" {
        return Ok(MaxSizeValue::None);
    }
    match parse_length_percentage(name, value, false)? {
        LengthPercentageValue::Px(value) => Ok(MaxSizeValue::Px(value)),
        LengthPercentageValue::Percent(value) => Ok(MaxSizeValue::Percent(value)),
    }
}

fn parse_length_percentage(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<LengthPercentageValue, StyleError> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent = parse_number(name, percent.trim())?;
        if !allow_negative && percent < 0.0 {
            return invalid(name, value);
        }
        return Ok(LengthPercentageValue::Percent(percent));
    }
    let pixels = parse_length(name, value)?;
    if !allow_negative && pixels < 0.0 {
        return invalid(name, value);
    }
    Ok(LengthPercentageValue::Px(pixels))
}

fn parse_length_value(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<LengthValue, StyleError> {
    let pixels = parse_length(name, value)?;
    if !allow_negative && pixels < 0.0 {
        return invalid(name, value);
    }
    Ok(LengthValue::Px(pixels))
}

fn parse_line_height(name: &str, value: &str) -> Result<LineHeightValue, StyleError> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent = parse_non_negative_number(name, percent.trim())?;
        return Ok(LineHeightValue::Percent(percent));
    }
    if let Some(pixels) = value.strip_suffix("px") {
        let pixels = parse_non_negative_number(name, pixels.trim())?;
        return Ok(LineHeightValue::Px(pixels));
    }
    if value == "normal" {
        return Ok(LineHeightValue::Normal);
    }
    Ok(LineHeightValue::Number(parse_non_negative_number(
        name, value,
    )?))
}

fn parse_box_length_percentages(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<[LengthPercentageValue; 4], StyleError> {
    parse_box_values(name, value, |part| {
        parse_length_percentage(name, part, allow_negative)
    })
}

fn parse_box_lengths(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<[LengthValue; 4], StyleError> {
    parse_box_values(name, value, |part| {
        parse_length_value(name, part, allow_negative)
    })
}

fn parse_box_sizes(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<[SizeValue; 4], StyleError> {
    parse_box_values(name, value, |part| parse_size(name, part, allow_negative))
}

fn parse_box_colors(name: &str, value: &str) -> Result<[Color; 4], StyleError> {
    parse_box_values(name, value, |part| parse_color(name, part))
}

fn parse_border_radius(name: &str, value: &str) -> Result<[CornerRadiusValue; 4], StyleError> {
    let mut parts = value.split('/');
    let horizontal = parts.next().unwrap_or_default().trim();
    let vertical = parts.next().map(str::trim);
    if parts.next().is_some() || horizontal.is_empty() || vertical.is_some_and(str::is_empty) {
        return invalid(name, value);
    }
    let horizontal = parse_box_length_percentages(name, horizontal, false)?;
    let vertical = match vertical {
        Some(vertical) => parse_box_length_percentages(name, vertical, false)?,
        None => horizontal,
    };
    Ok(std::array::from_fn(|index| {
        CornerRadiusValue::new(horizontal[index], vertical[index])
    }))
}

fn parse_corner_radius(name: &str, value: &str) -> Result<CornerRadiusValue, StyleError> {
    let (horizontal, vertical) = one_or_two(name, value, |name, part| {
        parse_length_percentage(name, part, false)
    })?;
    Ok(CornerRadiusValue::new(horizontal, vertical))
}

fn parse_box_values<T: Copy>(
    name: &str,
    value: &str,
    mut parse: impl FnMut(&str) -> Result<T, StyleError>,
) -> Result<[T; 4], StyleError> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    let values = parts
        .iter()
        .map(|part| parse(part))
        .collect::<Result<Vec<_>, _>>()?;
    match values.as_slice() {
        [all] => Ok([*all; 4]),
        [vertical, horizontal] => Ok([*vertical, *horizontal, *vertical, *horizontal]),
        [top, horizontal, bottom] => Ok([*top, *horizontal, *bottom, *horizontal]),
        [top, right, bottom, left] => Ok([*top, *right, *bottom, *left]),
        _ => invalid(name, value),
    }
}

fn one_or_two<T: Copy>(
    name: &str,
    value: &str,
    mut parse: impl FnMut(&str, &str) -> Result<T, StyleError>,
) -> Result<(T, T), StyleError> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        [both] => {
            let both = parse(name, both)?;
            Ok((both, both))
        }
        [first, second] => Ok((parse(name, first)?, parse(name, second)?)),
        _ => invalid(name, value),
    }
}

fn parse_overflow(name: &str, value: &str) -> Result<Overflow, StyleError> {
    match value {
        "visible" => Ok(Overflow::Visible),
        "hidden" => Ok(Overflow::Hidden),
        "clip" => Ok(Overflow::Clip),
        "auto" => Ok(Overflow::Auto),
        "scroll" => Ok(Overflow::Scroll),
        _ => invalid(name, value),
    }
}

fn parse_border_style(name: &str, value: &str) -> Result<BorderStyle, StyleError> {
    match value {
        "none" => Ok(BorderStyle::None),
        "hidden" => Ok(BorderStyle::Hidden),
        "dotted" => Ok(BorderStyle::Dotted),
        "dashed" => Ok(BorderStyle::Dashed),
        "solid" => Ok(BorderStyle::Solid),
        "double" => Ok(BorderStyle::Double),
        "groove" => Ok(BorderStyle::Groove),
        "ridge" => Ok(BorderStyle::Ridge),
        "inset" => Ok(BorderStyle::Inset),
        "outset" => Ok(BorderStyle::Outset),
        _ => invalid(name, value),
    }
}

fn parse_align_items(name: &str, value: &str) -> Result<AlignItems, StyleError> {
    match value {
        "start" => Ok(AlignItems::START),
        "end" => Ok(AlignItems::END),
        "flex-start" => Ok(AlignItems::FLEX_START),
        "flex-end" => Ok(AlignItems::FLEX_END),
        "center" => Ok(AlignItems::CENTER),
        "baseline" => Ok(AlignItems::BASELINE),
        "stretch" | "normal" => Ok(AlignItems::STRETCH),
        _ => invalid(name, value),
    }
}

fn parse_align_content(name: &str, value: &str) -> Result<AlignContent, StyleError> {
    match value {
        "start" => Ok(AlignContent::START),
        "end" => Ok(AlignContent::END),
        "flex-start" => Ok(AlignContent::FLEX_START),
        "flex-end" => Ok(AlignContent::FLEX_END),
        "center" => Ok(AlignContent::CENTER),
        "stretch" | "normal" => Ok(AlignContent::STRETCH),
        "space-between" => Ok(AlignContent::SPACE_BETWEEN),
        "space-around" => Ok(AlignContent::SPACE_AROUND),
        "space-evenly" => Ok(AlignContent::SPACE_EVENLY),
        _ => invalid(name, value),
    }
}

fn parse_aspect_ratio(name: &str, value: &str) -> Result<f32, StyleError> {
    let (width, height) = value
        .split_once('/')
        .map_or((value, "1"), |(width, height)| {
            (width.trim(), height.trim())
        });
    let width = parse_non_negative_number(name, width)?;
    let height = parse_non_negative_number(name, height)?;
    if width == 0.0 || height == 0.0 {
        return invalid(name, value);
    }
    Ok(width / height)
}

fn parse_non_negative_number(name: &str, value: &str) -> Result<f32, StyleError> {
    let number = parse_number(name, value)?;
    if number < 0.0 {
        invalid(name, value)
    } else {
        Ok(number)
    }
}

fn parse_length(name: &str, value: &str) -> Result<f32, StyleError> {
    parse_number(name, value.strip_suffix("px").unwrap_or(value).trim())
}

fn parse_number(name: &str, value: &str) -> Result<f32, StyleError> {
    value
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))
}

fn parse_color(name: &str, value: &str) -> Result<Color, StyleError> {
    let named = match value.to_ascii_lowercase().as_str() {
        "transparent" => Some([0, 0, 0, 0]),
        "black" => Some([0, 0, 0, 255]),
        "white" => Some([255, 255, 255, 255]),
        "red" => Some([255, 0, 0, 255]),
        "green" => Some([0, 128, 0, 255]),
        "blue" => Some([0, 0, 255, 255]),
        "gray" | "grey" => Some([128, 128, 128, 255]),
        "yellow" => Some([255, 255, 0, 255]),
        "magenta" | "fuchsia" => Some([255, 0, 255, 255]),
        "cyan" | "aqua" => Some([0, 255, 255, 255]),
        _ => None,
    };
    if let Some(color) = named {
        return Ok(color);
    }

    let hex = value
        .strip_prefix('#')
        .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?;
    let parse = |value: &str| {
        u8::from_str_radix(value, 16)
            .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))
    };
    match hex.len() {
        3 | 4 => {
            let mut channels = [0, 0, 0, 255];
            for (index, digit) in hex.as_bytes().iter().enumerate() {
                channels[index] = parse(&format!("{0}{0}", *digit as char))?;
            }
            Ok(channels)
        }
        6 | 8 => Ok([
            parse(&hex[0..2])?,
            parse(&hex[2..4])?,
            parse(&hex[4..6])?,
            if hex.len() == 8 {
                parse(&hex[6..8])?
            } else {
                255
            },
        ]),
        _ => invalid(name, value),
    }
}

fn invalid<T>(name: &str, value: &str) -> Result<T, StyleError> {
    Err(StyleError::InvalidValue(name.into(), value.into()))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum StyleError {
    #[error("unsupported style property '{0}'")]
    UnsupportedProperty(String),
    #[error("invalid value '{1}' for style property '{0}'")]
    InvalidValue(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_defaults_to_block() {
        assert_eq!(Style::default().display, Display::Block);
    }

    #[test]
    fn preserves_auto_and_always_visible_scroll_overflow() {
        let mut style = Style::default();
        set_style(&mut style, "overflow-x", Some("auto")).unwrap();
        set_style(&mut style, "overflow-y", Some("scroll")).unwrap();

        assert_eq!(style.overflow_x, Overflow::Auto);
        assert_eq!(style.overflow_y, Overflow::Scroll);
    }

    #[test]
    fn accepts_common_layout_properties_and_css_units() {
        let mut style = Style::default();
        set_style(&mut style, "display", Some("flex")).unwrap();
        set_style(&mut style, "width", Some("75%")).unwrap();
        set_style(&mut style, "margin", Some("10px auto 20px")).unwrap();
        set_style(&mut style, "paddingInline", Some("4px")).unwrap_err();
        set_style(&mut style, "alignItems", Some("center")).unwrap();

        assert_eq!(style.display, Display::Flex);
        assert_eq!(style.width, SizeValue::Percent(75.0));
        assert_eq!(style.margin_top, SizeValue::Px(10.0));
        assert_eq!(style.margin_right, SizeValue::Auto);
        assert_eq!(style.margin_bottom, SizeValue::Px(20.0));
        assert_eq!(style.margin_left, SizeValue::Auto);
        assert_eq!(style.align_items, Some(AlignItems::CENTER));
    }

    #[test]
    fn expands_box_and_gap_shorthands() {
        let mut style = Style::default();
        set_style(&mut style, "padding", Some("1px 2px 3px 4px")).unwrap();
        set_style(&mut style, "gap", Some("8px 12px")).unwrap();
        set_style(&mut style, "border-radius", Some("2px 4px")).unwrap();

        assert_eq!(
            [
                style.padding_top,
                style.padding_right,
                style.padding_bottom,
                style.padding_left,
            ],
            [
                LengthPercentageValue::Px(1.0),
                LengthPercentageValue::Px(2.0),
                LengthPercentageValue::Px(3.0),
                LengthPercentageValue::Px(4.0),
            ]
        );
        assert_eq!(style.row_gap, LengthPercentageValue::Px(8.0));
        assert_eq!(style.column_gap, LengthPercentageValue::Px(12.0));
        assert_eq!(
            style.border_top_left_radius,
            CornerRadiusValue::all(LengthPercentageValue::Px(2.0))
        );
        assert_eq!(
            style.border_top_right_radius,
            CornerRadiusValue::all(LengthPercentageValue::Px(4.0))
        );
        assert_eq!(
            style.border_bottom_right_radius,
            CornerRadiusValue::all(LengthPercentageValue::Px(2.0))
        );
        assert_eq!(
            style.border_bottom_left_radius,
            CornerRadiusValue::all(LengthPercentageValue::Px(4.0))
        );
    }

    #[test]
    fn parses_per_side_border_colors_styles_and_elliptical_radii() {
        let mut style = Style::default();

        set_style(
            &mut style,
            "border-color",
            Some("red green blue transparent"),
        )
        .unwrap();
        set_style(
            &mut style,
            "border-style",
            Some("solid dashed dotted double"),
        )
        .unwrap();
        set_style(
            &mut style,
            "border-radius",
            Some("10px 20% 30px / 5px 15% 25px 35%"),
        )
        .unwrap();

        assert_eq!(style.border_top_color, Some([255, 0, 0, 255]));
        assert_eq!(style.border_right_color, Some([0, 128, 0, 255]));
        assert_eq!(style.border_bottom_color, Some([0, 0, 255, 255]));
        assert_eq!(style.border_left_color, Some([0, 0, 0, 0]));
        assert_eq!(
            [
                style.border_top_style,
                style.border_right_style,
                style.border_bottom_style,
                style.border_left_style,
            ],
            [
                BorderStyle::Solid,
                BorderStyle::Dashed,
                BorderStyle::Dotted,
                BorderStyle::Double,
            ]
        );
        assert_eq!(
            style.border_top_left_radius,
            CornerRadiusValue::new(
                LengthPercentageValue::Px(10.0),
                LengthPercentageValue::Px(5.0),
            )
        );
        assert_eq!(
            style.border_bottom_left_radius,
            CornerRadiusValue::new(
                LengthPercentageValue::Percent(20.0),
                LengthPercentageValue::Percent(35.0),
            )
        );
        assert!(set_style(&mut style, "border-radius", Some("1px / 2px / 3px")).is_err());
    }

    #[test]
    fn preserves_all_four_position_values() {
        let mut style = Style::default();
        assert_eq!(style.position, Position::Static);

        for (value, expected) in [
            ("relative", Position::Relative),
            ("absolute", Position::Absolute),
            ("fixed", Position::Fixed),
            ("static", Position::Static),
        ] {
            set_style(&mut style, "position", Some(value)).unwrap();
            assert_eq!(style.position, expected);
        }
    }

    #[test]
    fn border_style_defaults_and_clears_to_none() {
        let mut style = Style::default();
        assert_eq!(
            [
                style.border_top_style,
                style.border_right_style,
                style.border_bottom_style,
                style.border_left_style,
            ],
            [BorderStyle::None; 4]
        );

        set_style(&mut style, "border-style", Some("solid")).unwrap();
        set_style(&mut style, "border-style", None).unwrap();

        assert_eq!(
            [
                style.border_top_style,
                style.border_right_style,
                style.border_bottom_style,
                style.border_left_style,
            ],
            [BorderStyle::None; 4]
        );
    }

    #[test]
    fn clears_properties_to_their_initial_values() {
        let mut style = Style::default();
        set_style(&mut style, "flex-shrink", Some("0")).unwrap();
        set_style(&mut style, "background-color", Some("#1234")).unwrap();
        set_style(&mut style, "z-index", Some("12")).unwrap();
        set_style(&mut style, "isolation", Some("isolate")).unwrap();
        set_style(&mut style, "flex-shrink", None).unwrap();
        set_style(&mut style, "background-color", Some("")).unwrap();
        set_style(&mut style, "z-index", None).unwrap();
        set_style(&mut style, "isolation", Some("")).unwrap();

        assert_eq!(style.flex_shrink, 1.0);
        assert_eq!(style.background_color, None);
        assert_eq!(style.z_index, ZIndex::Auto);
        assert_eq!(style.isolation, Isolation::Auto);
    }

    #[test]
    fn parses_z_index_and_isolation_as_enums() {
        let mut style = Style::default();

        set_style(&mut style, "zIndex", Some("-3")).unwrap();
        set_style(&mut style, "isolation", Some("isolate")).unwrap();

        assert_eq!(style.z_index, ZIndex::Value(-3));
        assert_eq!(style.isolation, Isolation::Isolate);

        assert!(set_style(&mut style, "z-index", Some("1.5")).is_err());
        assert!(set_style(&mut style, "isolation", Some("true")).is_err());
    }

    #[test]
    fn rejects_invalid_negative_box_sizes() {
        let mut style = Style::default();
        assert!(matches!(
            set_style(&mut style, "padding", Some("-1px")),
            Err(StyleError::InvalidValue(_, _))
        ));
    }

    #[test]
    fn property_types_reject_values_outside_their_css_grammar() {
        let mut style = Style::default();

        assert!(set_style(&mut style, "padding", Some("auto")).is_err());
        assert!(set_style(&mut style, "border-width", Some("10%")).is_err());
        assert!(set_style(&mut style, "max-width", Some("auto")).is_err());

        set_style(&mut style, "max-width", Some("none")).unwrap();
        set_style(&mut style, "line-height", Some("normal")).unwrap();
        assert_eq!(style.max_width, MaxSizeValue::None);
        assert_eq!(style.line_height, Some(LineHeightValue::Normal));
    }

    #[test]
    fn expands_flex_shorthand_and_parses_order() {
        let mut style = Style::default();

        set_style(&mut style, "flex", Some("2")).unwrap();
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (2.0, 1.0, SizeValue::Percent(0.0))
        );

        set_style(&mut style, "flex", Some("3 2 40%")).unwrap();
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (3.0, 2.0, SizeValue::Percent(40.0))
        );

        set_style(&mut style, "flex", Some("none")).unwrap();
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (0.0, 0.0, SizeValue::Auto)
        );

        set_style(&mut style, "flex", Some("25px")).unwrap();
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (1.0, 1.0, SizeValue::Px(25.0))
        );

        set_style(&mut style, "order", Some("-12")).unwrap();
        assert_eq!(style.order, -12);
        assert!(set_style(&mut style, "order", Some("1.5")).is_err());

        set_style(&mut style, "flex", None).unwrap();
        set_style(&mut style, "order", None).unwrap();
        assert_eq!(
            (style.flex_grow, style.flex_shrink, style.flex_basis),
            (0.0, 1.0, SizeValue::Auto)
        );
        assert_eq!(style.order, 0);
    }

    #[test]
    fn parses_grid_track_sizing_and_auto_flow_properties() {
        let mut style = Style::default();

        set_style(
            &mut style,
            "gridTemplateColumns",
            Some("[left] 80px [content] minmax(120px, 1fr) [right]"),
        )
        .unwrap();
        set_style(
            &mut style,
            "grid-template-rows",
            Some("repeat(2, minmax(20px, auto))"),
        )
        .unwrap();
        set_style(&mut style, "grid-auto-columns", Some("40px 10%")).unwrap();
        set_style(&mut style, "grid-auto-rows", Some("min-content 1fr")).unwrap();
        set_style(&mut style, "gridAutoFlow", Some("column dense")).unwrap();
        set_style(
            &mut style,
            "grid-template-areas",
            Some("\"header header\" \"sidebar main\""),
        )
        .unwrap();

        assert_eq!(
            style.grid_template_columns.as_deref(),
            Some("[left] 80px [content] minmax(120px, 1fr) [right]")
        );
        assert_eq!(
            style.grid_template_rows.as_deref(),
            Some("repeat(2, minmax(20px, auto))")
        );
        assert_eq!(style.grid_auto_columns.as_deref(), Some("40px 10%"));
        assert_eq!(style.grid_auto_rows.as_deref(), Some("min-content 1fr"));
        assert_eq!(
            style.grid_auto_flow,
            taffy::style::GridAutoFlow::ColumnDense
        );
        assert_eq!(
            style
                .grid_template_areas
                .iter()
                .map(|area| (
                    area.name.as_str(),
                    area.row_start,
                    area.row_end,
                    area.column_start,
                    area.column_end,
                ))
                .collect::<Vec<_>>(),
            vec![
                ("header", 1, 2, 1, 3),
                ("sidebar", 2, 3, 1, 2),
                ("main", 2, 3, 2, 3),
            ]
        );

        assert!(set_style(&mut style, "grid-auto-flow", Some("dense row column")).is_err());
        assert!(set_style(&mut style, "grid-template-rows", Some("-1fr")).is_err());
        assert!(set_style(
            &mut style,
            "grid-template-areas",
            Some("\"broken broken\" \"broken other\"")
        )
        .is_err());

        set_style(&mut style, "grid-template-columns", Some("none")).unwrap();
        set_style(&mut style, "grid-auto-columns", None).unwrap();
        set_style(&mut style, "grid-template-areas", Some("none")).unwrap();
        assert_eq!(style.grid_template_columns, None);
        assert_eq!(style.grid_auto_columns, None);
        assert!(style.grid_template_areas.is_empty());
    }

    #[test]
    fn parses_grid_template_and_placement_shorthands() {
        let mut style = Style::default();

        set_style(
            &mut style,
            "grid-template",
            Some("40px minmax(20px, auto) / 100px 1fr"),
        )
        .unwrap();
        assert_eq!(
            style.grid_template_rows.as_deref(),
            Some("40px minmax(20px, auto)")
        );
        assert_eq!(style.grid_template_columns.as_deref(), Some("100px 1fr"));

        set_style(
            &mut style,
            "grid-template",
            Some("\"header header\" 40px \"sidebar main\" minmax(60px, auto) / 100px 1fr"),
        )
        .unwrap();
        assert_eq!(
            style.grid_template_rows.as_deref(),
            Some("40px minmax(60px, auto)")
        );
        assert_eq!(style.grid_template_columns.as_deref(), Some("100px 1fr"));
        assert_eq!(
            style
                .grid_template_areas
                .iter()
                .map(|area| area.name.as_str())
                .collect::<Vec<_>>(),
            vec!["header", "sidebar", "main"]
        );
        set_style(&mut style, "grid-template", Some("none")).unwrap();
        assert_eq!(style.grid_template_rows, None);
        assert_eq!(style.grid_template_columns, None);
        assert!(style.grid_template_areas.is_empty());

        set_style(&mut style, "grid-row", Some("2 / span 3")).unwrap();
        set_style(&mut style, "grid-column-start", Some("content")).unwrap();
        set_style(&mut style, "grid-column-end", Some("-1")).unwrap();
        assert_eq!(style.grid_row_start.as_deref(), Some("2"));
        assert_eq!(style.grid_row_end.as_deref(), Some("span 3"));
        assert_eq!(style.grid_column_start.as_deref(), Some("content"));
        assert_eq!(style.grid_column_end.as_deref(), Some("-1"));

        set_style(&mut style, "grid-area", Some("2 / 3 / span 2 / 5")).unwrap();
        assert_eq!(style.grid_row_start.as_deref(), Some("2"));
        assert_eq!(style.grid_column_start.as_deref(), Some("3"));
        assert_eq!(style.grid_row_end.as_deref(), Some("span 2"));
        assert_eq!(style.grid_column_end.as_deref(), Some("5"));

        set_style(&mut style, "grid-area", Some("hero")).unwrap();
        assert_eq!(style.grid_row_start.as_deref(), Some("hero"));
        assert_eq!(style.grid_column_start.as_deref(), Some("hero"));
        assert_eq!(style.grid_row_end.as_deref(), Some("hero"));
        assert_eq!(style.grid_column_end.as_deref(), Some("hero"));

        assert!(set_style(&mut style, "grid-row", Some("1 / 2 / 3")).is_err());
        assert!(set_style(&mut style, "grid-area", Some("0")).is_err());
    }
}
