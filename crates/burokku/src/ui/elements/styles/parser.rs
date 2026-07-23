use std::borrow::Cow;

use super::{
    AlignContent, AlignItems, BoxSizing, Color, Display, FlexDirection, FlexWrap,
    LengthPercentageValue, LengthValue, LineHeightValue, MaxSizeValue, Overflow, Position,
    SizeValue, Style,
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
                "relative" | "static" => Position::Relative,
                "absolute" | "fixed" => Position::Absolute,
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

        "background-color" => color!(background_color),
        "color" => color!(color),
        "border-color" => color!(border_color),
        "border-radius" => {
            let [top_left, top_right, bottom_right, bottom_left] =
                parse_box_length_percentages(name, value, false)?;
            style.border_top_left_radius = top_left;
            style.border_top_right_radius = top_right;
            style.border_bottom_right_radius = bottom_right;
            style.border_bottom_left_radius = bottom_left;
        }
        "border-top-left-radius" => {
            style.border_top_left_radius = parse_length_percentage(name, value, false)?
        }
        "border-top-right-radius" => {
            style.border_top_right_radius = parse_length_percentage(name, value, false)?
        }
        "border-bottom-right-radius" => {
            style.border_bottom_right_radius = parse_length_percentage(name, value, false)?
        }
        "border-bottom-left-radius" => {
            style.border_bottom_left_radius = parse_length_percentage(name, value, false)?
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
        "align-items" => reset!(align_items),
        "align-self" => reset!(align_self),
        "align-content" => reset!(align_content),
        "justify-content" => reset!(justify_content),
        "background-color" => reset!(background_color),
        "color" => reset!(color),
        "border-color" => reset!(border_color),
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
        "scroll" | "auto" => Ok(Overflow::Scroll),
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
        assert_eq!(style.border_top_left_radius, LengthPercentageValue::Px(2.0));
        assert_eq!(
            style.border_top_right_radius,
            LengthPercentageValue::Px(4.0)
        );
        assert_eq!(
            style.border_bottom_right_radius,
            LengthPercentageValue::Px(2.0)
        );
        assert_eq!(
            style.border_bottom_left_radius,
            LengthPercentageValue::Px(4.0)
        );
    }

    #[test]
    fn clears_properties_to_their_initial_values() {
        let mut style = Style::default();
        set_style(&mut style, "flex-shrink", Some("0")).unwrap();
        set_style(&mut style, "background-color", Some("#1234")).unwrap();
        set_style(&mut style, "flex-shrink", None).unwrap();
        set_style(&mut style, "background-color", Some("")).unwrap();

        assert_eq!(style.flex_shrink, 1.0);
        assert_eq!(style.background_color, None);
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
}
