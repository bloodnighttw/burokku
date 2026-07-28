use super::super::Style;
use super::{invalid, StyleError};
use taffy::style::{
    GridPlacement, GridTemplateArea, GridTemplateComponent, GridTemplateTracks, TrackSizingFunction,
};

pub(super) fn set_grid_template_rows(
    style: &mut Style,
    name: &str,
    value: &str,
) -> Result<(), StyleError> {
    style.grid_template_rows = parse_grid_template(name, value)?;
    Ok(())
}

pub(super) fn set_grid_template_columns(
    style: &mut Style,
    name: &str,
    value: &str,
) -> Result<(), StyleError> {
    style.grid_template_columns = parse_grid_template(name, value)?;
    Ok(())
}

type TemplateTracks = GridTemplateTracks<String, GridTemplateComponent<String>>;

pub(super) fn parse_grid_template(name: &str, value: &str) -> Result<Option<String>, StyleError> {
    if value == "none" {
        return Ok(None);
    }
    parse_css::<TemplateTracks>(name, value)?;
    Ok(Some(value.into()))
}

pub(super) fn split_grid_shorthand<'a>(
    name: &str,
    value: &'a str,
) -> Result<(&'a str, &'a str), StyleError> {
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

pub(super) fn parse_grid_template_areas(
    name: &str,
    value: &str,
) -> Result<Vec<GridTemplateArea<String>>, StyleError> {
    if value == "none" {
        return Ok(Vec::new());
    }

    build_grid_template_areas(name, value, parse_quoted_grid_rows(name, value)?)
}

pub(super) fn build_grid_template_areas(
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

pub(super) fn parse_grid_area_template_rows(
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

pub(super) fn skip_ascii_whitespace(value: &str, mut cursor: usize) -> usize {
    while cursor < value.len() && value.as_bytes()[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

pub(super) fn parse_quoted_grid_rows(
    name: &str,
    value: &str,
) -> Result<Vec<Vec<String>>, StyleError> {
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

pub(super) fn valid_grid_area_name(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(
        characters.next(),
        Some(character) if character.is_ascii_alphabetic() || character == '_' || character == '-'
    ) && characters
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

pub(super) fn parse_grid_placement(name: &str, value: &str) -> Result<Option<String>, StyleError> {
    parse_css::<GridPlacement<String>>(name, value)?;
    Ok((value != "auto").then(|| value.into()))
}

pub(super) fn parse_grid_axis(
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

pub(super) fn parse_grid_area(name: &str, value: &str) -> Result<[Option<String>; 4], StyleError> {
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

pub(super) fn repeated_named_placement(value: &Option<String>) -> Option<String> {
    let value = value.as_deref()?;
    matches!(
        parse_css::<GridPlacement<String>>("grid-area", value).ok()?,
        GridPlacement::NamedLine(_, _)
    )
    .then(|| value.into())
}

pub(super) fn parse_css<T>(name: &str, value: &str) -> Result<T, StyleError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))
}
