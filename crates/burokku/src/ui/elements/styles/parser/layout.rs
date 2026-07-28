use super::super::{
    AlignContent, AlignItems, LengthPercentageValue, LengthValue, LineHeightValue, MaxSizeValue,
    Overflow, SizeValue,
};
use super::{invalid, StyleError};

pub(super) fn parse_flex(name: &str, value: &str) -> Result<(f32, f32, SizeValue), StyleError> {
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

pub(super) fn parse_size(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<SizeValue, StyleError> {
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

pub(super) fn parse_max_size(name: &str, value: &str) -> Result<MaxSizeValue, StyleError> {
    if value == "none" {
        return Ok(MaxSizeValue::None);
    }
    match parse_length_percentage(name, value, false)? {
        LengthPercentageValue::Px(value) => Ok(MaxSizeValue::Px(value)),
        LengthPercentageValue::Percent(value) => Ok(MaxSizeValue::Percent(value)),
    }
}

pub(super) fn parse_length_percentage(
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

pub(super) fn parse_length_value(
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

pub(super) fn parse_line_height(name: &str, value: &str) -> Result<LineHeightValue, StyleError> {
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

pub(super) fn parse_box_length_percentages(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<[LengthPercentageValue; 4], StyleError> {
    parse_box_values(name, value, |part| {
        parse_length_percentage(name, part, allow_negative)
    })
}

pub(super) fn parse_box_lengths(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<[LengthValue; 4], StyleError> {
    parse_box_values(name, value, |part| {
        parse_length_value(name, part, allow_negative)
    })
}

pub(super) fn parse_box_sizes(
    name: &str,
    value: &str,
    allow_negative: bool,
) -> Result<[SizeValue; 4], StyleError> {
    parse_box_values(name, value, |part| parse_size(name, part, allow_negative))
}

pub(super) fn parse_box_values<T: Copy>(
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

pub(super) fn one_or_two<T: Copy>(
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

pub(super) fn parse_overflow(name: &str, value: &str) -> Result<Overflow, StyleError> {
    match value {
        "visible" => Ok(Overflow::Visible),
        "hidden" => Ok(Overflow::Hidden),
        "clip" => Ok(Overflow::Clip),
        "auto" => Ok(Overflow::Auto),
        "scroll" => Ok(Overflow::Scroll),
        _ => invalid(name, value),
    }
}

pub(super) fn parse_align_items(name: &str, value: &str) -> Result<AlignItems, StyleError> {
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

pub(super) fn parse_align_content(name: &str, value: &str) -> Result<AlignContent, StyleError> {
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

pub(super) fn parse_aspect_ratio(name: &str, value: &str) -> Result<f32, StyleError> {
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

pub(super) fn parse_non_negative_number(name: &str, value: &str) -> Result<f32, StyleError> {
    let number = parse_number(name, value)?;
    if number < 0.0 {
        invalid(name, value)
    } else {
        Ok(number)
    }
}

pub(super) fn parse_length(name: &str, value: &str) -> Result<f32, StyleError> {
    parse_number(name, value.strip_suffix("px").unwrap_or(value).trim())
}

pub(super) fn parse_number(name: &str, value: &str) -> Result<f32, StyleError> {
    value
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))
}
