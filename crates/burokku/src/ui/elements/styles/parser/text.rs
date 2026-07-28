use super::super::{Style, TextDecorationLineValue};
use super::{invalid, paint::parse_color, StyleError};

pub(super) fn parse_font_families(name: &str, value: &str) -> Result<Vec<String>, StyleError> {
    let mut families = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (index, character) in value.char_indices() {
        match (quote, character) {
            (Some(expected), actual) if actual == expected => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, ',') => {
                let family = value[start..index].trim().trim_matches(['\'', '"']).trim();
                if family.is_empty() {
                    return invalid(name, value);
                }
                families.push(family.to_owned());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return invalid(name, value);
    }
    let family = value[start..].trim().trim_matches(['\'', '"']).trim();
    if family.is_empty() {
        return invalid(name, value);
    }
    families.push(family.to_owned());
    Ok(families)
}

pub(super) fn parse_text_decoration(
    style: &mut Style,
    name: &str,
    value: &str,
) -> Result<(), StyleError> {
    let mut line = TextDecorationLineValue::NONE;
    let mut saw_line = false;
    let mut color = None;
    for token in value.split_ascii_whitespace() {
        let flag = match token {
            "none" if !saw_line => {
                saw_line = true;
                TextDecorationLineValue::NONE
            }
            "underline" => TextDecorationLineValue::UNDERLINE,
            "overline" => TextDecorationLineValue::OVERLINE,
            "line-through" => TextDecorationLineValue::LINE_THROUGH,
            _ if color.is_none() => {
                color = Some(parse_color(name, token)?);
                continue;
            }
            _ => return invalid(name, value),
        };
        if token != "none" {
            saw_line = true;
            line = line.union(flag);
        }
    }
    if !saw_line {
        return invalid(name, value);
    }
    style.text_decoration_line = Some(line);
    if let Some(color) = color {
        style.text_decoration_color = Some(color);
    }
    Ok(())
}

pub(super) fn parse_text_decoration_line(
    name: &str,
    value: &str,
) -> Result<TextDecorationLineValue, StyleError> {
    let mut line = TextDecorationLineValue::NONE;
    let mut saw_value = false;
    for token in value.split_ascii_whitespace() {
        let flag = match token {
            "none" if !saw_value => TextDecorationLineValue::NONE,
            "underline" => TextDecorationLineValue::UNDERLINE,
            "overline" => TextDecorationLineValue::OVERLINE,
            "line-through" => TextDecorationLineValue::LINE_THROUGH,
            _ => return invalid(name, value),
        };
        saw_value = true;
        line = line.union(flag);
    }
    if !saw_value {
        return invalid(name, value);
    }
    Ok(line)
}
