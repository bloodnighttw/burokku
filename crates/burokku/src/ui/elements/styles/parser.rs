use std::borrow::Cow;

use super::{
    BoxSizing, Display, FlexDirection, FlexWrap, FontStyleValue, Isolation, LengthValue,
    OverflowWrapValue, Position, Style, TextAlignValue, WhiteSpaceValue, WordBreakValue, ZIndex,
};
use taffy::style::GridAutoTracks;
use thiserror::Error;

mod grid;
mod layout;
mod paint;
mod text;

use grid::*;
use layout::*;
use paint::*;
use text::*;

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
            }
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
        "background-image" => style.background_image = parse_background_image(name, value)?,
        "color" => color!(color),
        "opacity" => {
            style.opacity = if let Some(percent) = value.strip_suffix('%') {
                parse_number(name, percent.trim())? / 100.0
            } else {
                parse_number(name, value)?
            };
            if !(0.0..=1.0).contains(&style.opacity) {
                return invalid(name, value);
            }
        }
        "transform" => style.transform = parse_transform(name, value)?,
        "box-shadow" => style.box_shadow = parse_shadow(name, value, true)?,
        "text-shadow" => style.text_shadow = parse_shadow(name, value, false)?,
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
        "font-family" => style.font_families = Some(parse_font_families(name, value)?),
        "font-style" => {
            style.font_style = Some(match value {
                "normal" => FontStyleValue::Normal,
                "italic" => FontStyleValue::Italic,
                "oblique" => FontStyleValue::Oblique,
                _ => return invalid(name, value),
            })
        }
        "text-align" => {
            style.text_align = Some(match value {
                "start" => TextAlignValue::Start,
                "end" => TextAlignValue::End,
                "left" => TextAlignValue::Left,
                "right" => TextAlignValue::Right,
                "center" => TextAlignValue::Center,
                "justify" => TextAlignValue::Justify,
                _ => return invalid(name, value),
            })
        }
        "letter-spacing" => {
            style.letter_spacing = Some(if value == "normal" {
                LengthValue::ZERO
            } else {
                parse_length_value(name, value, true)?
            })
        }
        "word-spacing" => {
            style.word_spacing = Some(if value == "normal" {
                LengthValue::ZERO
            } else {
                parse_length_value(name, value, true)?
            })
        }
        "text-decoration" => parse_text_decoration(style, name, value)?,
        "text-decoration-line" => {
            style.text_decoration_line = Some(parse_text_decoration_line(name, value)?)
        }
        "text-decoration-color" => style.text_decoration_color = Some(parse_color(name, value)?),
        "white-space" => {
            style.white_space = Some(match value {
                "normal" => WhiteSpaceValue::Normal,
                "nowrap" => WhiteSpaceValue::NoWrap,
                "pre" => WhiteSpaceValue::Pre,
                "pre-wrap" => WhiteSpaceValue::PreWrap,
                "pre-line" => WhiteSpaceValue::PreLine,
                "break-spaces" => WhiteSpaceValue::BreakSpaces,
                _ => return invalid(name, value),
            })
        }
        "overflow-wrap" | "word-wrap" => {
            style.overflow_wrap = Some(match value {
                "normal" => OverflowWrapValue::Normal,
                "break-word" => OverflowWrapValue::BreakWord,
                "anywhere" => OverflowWrapValue::Anywhere,
                _ => return invalid(name, value),
            })
        }
        "word-break" => {
            style.word_break = Some(match value {
                "normal" => WordBreakValue::Normal,
                "break-all" => WordBreakValue::BreakAll,
                "keep-all" => WordBreakValue::KeepAll,
                _ => return invalid(name, value),
            })
        }
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
        "background-image" => reset!(background_image),
        "color" => reset!(color),
        "opacity" => reset!(opacity),
        "transform" => reset!(transform),
        "box-shadow" => reset!(box_shadow),
        "text-shadow" => reset!(text_shadow),
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
        "font-family" => reset!(font_families),
        "font-style" => reset!(font_style),
        "text-align" => reset!(text_align),
        "letter-spacing" => reset!(letter_spacing),
        "word-spacing" => reset!(word_spacing),
        "text-decoration" => {
            reset!(text_decoration_line);
            reset!(text_decoration_color);
        }
        "text-decoration-line" => reset!(text_decoration_line),
        "text-decoration-color" => reset!(text_decoration_color),
        "white-space" => reset!(white_space),
        "overflow-wrap" | "word-wrap" => reset!(overflow_wrap),
        "word-break" => reset!(word_break),
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
    use taffy::AlignItems;

    use crate::ui::elements::styles::{
        BackgroundImage, GradientStop, LengthPercentageValue, LineHeightValue, MaxSizeValue,
        Overflow, SizeValue, TextDecorationLineValue, Transform,
    };

    use super::*;

    #[test]
    fn display_defaults_to_block() {
        assert_eq!(Style::default().display, Display::Block);
    }

    #[test]
    fn preserves_css_position_values() {
        let mut style = Style::default();
        assert_eq!(style.position, Position::Static);

        set_style(&mut style, "position", Some("relative")).unwrap();
        assert_eq!(style.position, Position::Relative);

        set_style(&mut style, "position", Some("static")).unwrap();
        assert_eq!(style.position, Position::Static);

        set_style(&mut style, "position", Some("absolute")).unwrap();
        assert_eq!(style.position, Position::Absolute);

        set_style(&mut style, "position", Some("fixed")).unwrap();
        assert_eq!(style.position, Position::Fixed);

        set_style(&mut style, "position", None).unwrap();
        assert_eq!(style.position, Position::Static);
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
    fn parses_typography_properties_and_font_fallbacks() {
        let mut style = Style::default();
        set_style(
            &mut style,
            "font-family",
            Some("\"Inter\", Noto Sans, sans-serif"),
        )
        .unwrap();
        set_style(&mut style, "font-style", Some("italic")).unwrap();
        set_style(&mut style, "text-align", Some("center")).unwrap();
        set_style(&mut style, "letter-spacing", Some("-0.5px")).unwrap();
        set_style(&mut style, "word-spacing", Some("3px")).unwrap();
        set_style(
            &mut style,
            "text-decoration",
            Some("underline line-through red"),
        )
        .unwrap();
        set_style(&mut style, "white-space", Some("pre-wrap")).unwrap();
        set_style(&mut style, "overflow-wrap", Some("anywhere")).unwrap();

        assert_eq!(
            style.font_families,
            Some(vec![
                "Inter".to_owned(),
                "Noto Sans".to_owned(),
                "sans-serif".to_owned()
            ])
        );
        assert_eq!(style.font_style, Some(FontStyleValue::Italic));
        assert_eq!(style.text_align, Some(TextAlignValue::Center));
        assert_eq!(style.letter_spacing, Some(LengthValue::Px(-0.5)));
        assert_eq!(style.word_spacing, Some(LengthValue::Px(3.0)));
        let decoration = style.text_decoration_line.unwrap();
        assert!(decoration.contains(TextDecorationLineValue::UNDERLINE));
        assert!(decoration.contains(TextDecorationLineValue::LINE_THROUGH));
        assert_eq!(style.text_decoration_color, Some([255, 0, 0, 255]));
        assert_eq!(style.white_space, Some(WhiteSpaceValue::PreWrap));
        assert_eq!(style.overflow_wrap, Some(OverflowWrapValue::Anywhere));
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
    fn parses_functional_and_extended_named_colors() {
        let mut style = Style::default();
        set_style(&mut style, "color", Some("rgb(100% 0% 50% / 25%)")).unwrap();
        assert_eq!(style.color, Some([255, 0, 128, 64]));

        set_style(&mut style, "color", Some("rgba(12, 34, 56, 0.5)")).unwrap();
        assert_eq!(style.color, Some([12, 34, 56, 128]));

        set_style(&mut style, "color", Some("hsl(120 100% 25%)")).unwrap();
        assert_eq!(style.color, Some([0, 128, 0, 255]));

        set_style(&mut style, "color", Some("rebeccapurple")).unwrap();
        assert_eq!(style.color, Some([102, 51, 153, 255]));
        set_style(&mut style, "color", Some("lightgoldenrodyellow")).unwrap();
        assert_eq!(style.color, Some([250, 250, 210, 255]));
    }

    #[test]
    fn parses_opacity_transform_and_shadows() {
        let mut style = Style::default();
        set_style(&mut style, "opacity", Some("0.35")).unwrap();
        set_style(
            &mut style,
            "transform",
            Some("translate(10px, 20px) scale(2) rotate(0deg)"),
        )
        .unwrap();
        set_style(
            &mut style,
            "box-shadow",
            Some("4px 6px 8px 2px rgba(0, 0, 0, 0.5)"),
        )
        .unwrap();
        set_style(&mut style, "text-shadow", Some("1px 2px 3px navy")).unwrap();

        assert_eq!(style.opacity, 0.35);
        assert_eq!(
            style.transform,
            Transform::Matrix([2.0, 0.0, 0.0, 2.0, 10.0, 20.0])
        );
        assert_eq!(style.box_shadow[0].spread, 2.0);
        assert_eq!(style.box_shadow[0].color, [0, 0, 0, 128]);
        assert_eq!(style.text_shadow[0].color, [0, 0, 128, 255]);
        assert!(set_style(&mut style, "opacity", Some("1.1")).is_err());
        assert!(set_style(&mut style, "text-shadow", Some("1px 2px 3px 4px red")).is_err());

        set_style(&mut style, "opacity", Some("35%")).unwrap();
        set_style(
            &mut style,
            "box-shadow",
            Some("inset 1px 2px 3px red, 4px 5px blue"),
        )
        .unwrap();
        set_style(
            &mut style,
            "text-shadow",
            Some("1px 2px red, 3px 4px 5px blue"),
        )
        .unwrap();
        assert_eq!(style.opacity, 0.35);
        assert_eq!(style.box_shadow.len(), 2);
        assert!(style.box_shadow[0].inset);
        assert!(!style.box_shadow[1].inset);
        assert_eq!(style.text_shadow.len(), 2);

        set_style(&mut style, "transform", Some("skewX(45deg)")).unwrap();
        assert!((style.transform.matrix()[2] - 1.0).abs() < 0.0001);

        set_style(&mut style, "transform", Some("none")).unwrap();
        assert_eq!(style.transform, Transform::None);

        set_style(&mut style, "transform", Some("translateX(0px)")).unwrap();
        assert_eq!(
            style.transform,
            Transform::Matrix(Transform::IDENTITY_MATRIX)
        );
    }

    #[test]
    fn parses_linear_and_radial_gradient_images() {
        let mut style = Style::default();
        set_style(
            &mut style,
            "background-image",
            Some("linear-gradient(to right, red 0%, rgb(0 0 255) 100%)"),
        )
        .unwrap();
        assert_eq!(
            style.background_image,
            Some(BackgroundImage::LinearGradient {
                direction: [1.0, 0.0],
                stops: vec![
                    GradientStop {
                        color: [255, 0, 0, 255],
                        position: 0.0,
                    },
                    GradientStop {
                        color: [0, 0, 255, 255],
                        position: 1.0,
                    },
                ],
            })
        );

        set_style(
            &mut style,
            "background-image",
            Some("radial-gradient(white, transparent)"),
        )
        .unwrap();
        assert_eq!(
            style.background_image,
            Some(BackgroundImage::RadialGradient {
                stops: vec![
                    GradientStop {
                        color: [255, 255, 255, 255],
                        position: 0.0,
                    },
                    GradientStop {
                        color: [0, 0, 0, 0],
                        position: 1.0,
                    },
                ],
            })
        );

        set_style(
            &mut style,
            "background-image",
            Some("linear-gradient(red 10%, yellow, lime 70%, blue)"),
        )
        .unwrap();
        let Some(BackgroundImage::LinearGradient { stops, .. }) = style.background_image else {
            panic!("expected linear gradient");
        };
        assert_eq!(stops.len(), 4);
        for (actual, expected) in stops
            .iter()
            .map(|stop| stop.position)
            .zip([0.1, 0.4, 0.7, 1.0])
        {
            assert!((actual - expected).abs() < 0.0001);
        }
    }

    #[test]
    fn decodes_and_caches_png_data_url_backgrounds() {
        const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
        let mut first = Style::default();
        let mut second = Style::default();
        set_style(
            &mut first,
            "background-image",
            Some(&format!("url(\"{PNG}\")")),
        )
        .unwrap();
        set_style(
            &mut second,
            "background-image",
            Some(&format!("url('{PNG}')")),
        )
        .unwrap();

        let Some(BackgroundImage::Raster(first)) = first.background_image else {
            panic!("expected decoded raster image");
        };
        let Some(BackgroundImage::Raster(second)) = second.background_image else {
            panic!("expected cached raster image");
        };
        assert_eq!((first.width, first.height), (2, 1));
        assert_eq!(&*first.pixels, &[255, 0, 0, 255, 0, 0, 255, 255]);
        assert!(std::sync::Arc::ptr_eq(&first.pixels, &second.pixels));
        assert!(set_style(
            &mut Style::default(),
            "background-image",
            Some("url(https://example.com/image.png)")
        )
        .is_err());
    }

    #[test]
    fn rejects_pngs_exceeding_decoded_dimension_limits_before_pixel_allocation() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 4097, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![0; 4097 * 4]).unwrap();
        }
        assert!(decode_png(&encoded).is_none());
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
