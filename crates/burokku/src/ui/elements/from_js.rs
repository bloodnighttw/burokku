use std::str::FromStr;

use render::{
    BackgroundImage, Color, FontFamily, FontStyle, GradientStop, RasterImage, TextAlign,
    TextDecorationLine, TextOverflowWrap, TextWhiteSpace, TextWordBreak, TextWrap,
};
use runtime::rquickjs::{Array, Ctx, Error, FromJs, Object, Result, Value};
use taffy::{
    geometry::{Line, Size},
    style::{GridAutoTracks, GridTemplateTracks},
    AlignContent, AlignItems, GridAutoFlow, GridPlacement, GridTemplateComponent,
};

use super::{
    styles::{
        div::DivStyle,
        flex::{FlexBasis, FlexStyle},
        grid::{GridStyle, GridTemplate, GridTrackSizing},
        shared::{background::Background, border::Border, corner_radius::CornerRadius},
        text::{LineHeight, TextDecorationColor, TextStyle},
    },
    Elements,
};

impl<'js> FromJs<'js> for Elements {
    fn from_js(context: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        element_from_js(context, value, None)
    }
}

fn element_from_js<'js>(
    context: &Ctx<'js>,
    value: Value<'js>,
    inherited_text_style: Option<&TextStyle>,
) -> Result<Elements> {
    let object = Object::from_js(context, value)?;
    let element_type: String = object.get("type")?;

    match element_type.as_str() {
        "app" => Ok(Elements::App {
            children: children_from_js(context, &object, None)?,
        }),
        "window" => Ok(Elements::Window {
            children: children_from_js(context, &object, None)?,
        }),
        "div" => Ok(Elements::Div {
            style: Box::new(div_style_from_js(&object)?),
            children: children_from_js(context, &object, None)?,
        }),
        "flex" => Ok(Elements::Flex {
            style: Box::new(flex_style_from_js(&object)?),
            children: children_from_js(context, &object, None)?,
        }),
        "grid" => Ok(Elements::Grid {
            style: Box::new(grid_style_from_js(&object)?),
            children: children_from_js(context, &object, None)?,
        }),
        "text" => {
            let style = Box::new(text_style_from_js(&object, inherited_text_style)?);
            let children = children_from_js(context, &object, Some(&style))?;
            Ok(Elements::Text { style, children })
        }
        "string" => Ok(Elements::_String {
            string: object.get("value")?,
        }),
        _ => Err(decode_error(format!(
            "unsupported element type {element_type:?}"
        ))),
    }
}

fn children_from_js<'js>(
    context: &Ctx<'js>,
    object: &Object<'js>,
    inherited_text_style: Option<&TextStyle>,
) -> Result<Vec<Elements>> {
    let Some(children) = object.get::<_, Option<Array<'js>>>("children")? else {
        return Ok(Vec::new());
    };

    children
        .iter::<Value<'js>>()
        .map(|child| element_from_js(context, child?, inherited_text_style))
        .collect()
}

fn style_object<'js>(element: &Object<'js>) -> Result<Option<Object<'js>>> {
    element.get("style")
}

fn property<'js, T>(style: Option<&Object<'js>>, name: &'static str) -> Result<Option<T>>
where
    T: FromJs<'js>,
{
    match style {
        Some(style) => style.get(name),
        None => Ok(None),
    }
}

fn div_style_from_js(element: &Object<'_>) -> Result<DivStyle> {
    let object = style_object(element)?;
    let mut style = DivStyle::default();
    apply_box_paint(
        "div",
        object.as_ref(),
        &mut style.background,
        &mut style.border,
        &mut style.corner_radius,
    )?;
    Ok(style)
}

fn flex_style_from_js(element: &Object<'_>) -> Result<FlexStyle> {
    let object = style_object(element)?;
    let object = object.as_ref();
    let mut style = FlexStyle::default();

    if let Some(value) = property::<String>(object, "flexDirection")? {
        style.direction = parse_css("flex", "flexDirection", &value)?;
    }
    if let Some(value) = property::<String>(object, "flexWrap")? {
        style.wrap = parse_css("flex", "flexWrap", &value)?;
    }
    apply_gap(
        "flex",
        &mut style.gap,
        property(object, "gap")?,
        property(object, "rowGap")?,
        property(object, "columnGap")?,
    )?;
    if let Some(value) = property::<String>(object, "alignContent")? {
        style.align_content = Some(parse_content_alignment("flex", "alignContent", &value)?);
    }
    if let Some(value) = property::<String>(object, "alignItems")? {
        style.align_items = Some(parse_item_alignment("flex", "alignItems", &value)?);
    }
    if let Some(value) = property::<String>(object, "justifyContent")? {
        style.justify_content = Some(parse_content_alignment("flex", "justifyContent", &value)?);
    }
    if let Some(value) = property::<Value<'_>>(object, "flexBasis")? {
        style.basis = if let Some(number) = value.as_number() {
            FlexBasis::Length(non_negative("flex", "flexBasis", number as f32)?)
        } else if value.is_string() {
            let keyword = value.get::<String>()?;
            if keyword == "auto" {
                FlexBasis::Auto
            } else {
                return Err(invalid(
                    "flex",
                    "flexBasis",
                    quoted(&keyword),
                    "expected a non-negative number or \"auto\"",
                ));
            }
        } else {
            return Err(invalid(
                "flex",
                "flexBasis",
                value.type_of().as_str(),
                "expected a non-negative number or \"auto\"",
            ));
        };
    }
    if let Some(value) = property::<f32>(object, "flexGrow")? {
        style.grow = non_negative("flex", "flexGrow", value)?;
    }
    if let Some(value) = property::<f32>(object, "flexShrink")? {
        style.shrink = non_negative("flex", "flexShrink", value)?;
    }
    if let Some(value) = property::<String>(object, "alignSelf")? {
        style.align_self = Some(parse_item_alignment("flex", "alignSelf", &value)?);
    }

    apply_box_paint(
        "flex",
        object,
        &mut style.background,
        &mut style.border,
        &mut style.corner_radius,
    )?;
    Ok(style)
}

fn grid_style_from_js(element: &Object<'_>) -> Result<GridStyle> {
    let object = style_object(element)?;
    let object = object.as_ref();
    let mut style = GridStyle::default();

    if let Some(value) = property::<String>(object, "gridTemplateColumns")? {
        style.template_columns = parse_grid_template("gridTemplateColumns", &value)?;
    }
    if let Some(value) = property::<String>(object, "gridTemplateRows")? {
        style.template_rows = parse_grid_template("gridTemplateRows", &value)?;
    }
    if let Some(value) = property::<String>(object, "gridAutoColumns")? {
        style.auto_columns = parse_grid_auto_tracks("gridAutoColumns", &value)?;
    }
    if let Some(value) = property::<String>(object, "gridAutoRows")? {
        style.auto_rows = parse_grid_auto_tracks("gridAutoRows", &value)?;
    }
    if let Some(value) = property::<String>(object, "gridAutoFlow")? {
        style.auto_flow = match value.as_str() {
            "row" => GridAutoFlow::Row,
            "column" => GridAutoFlow::Column,
            "row-dense" => GridAutoFlow::RowDense,
            "column-dense" => GridAutoFlow::ColumnDense,
            _ => {
                return Err(invalid(
                    "grid",
                    "gridAutoFlow",
                    quoted(&value),
                    "expected row, column, row-dense, or column-dense",
                ));
            }
        };
    }
    apply_gap(
        "grid",
        &mut style.gap,
        property(object, "gap")?,
        property(object, "rowGap")?,
        property(object, "columnGap")?,
    )?;
    if let Some(value) = property::<String>(object, "alignContent")? {
        style.align_content = Some(parse_content_alignment("grid", "alignContent", &value)?);
    }
    if let Some(value) = property::<String>(object, "justifyContent")? {
        style.justify_content = Some(parse_content_alignment("grid", "justifyContent", &value)?);
    }
    if let Some(value) = property::<String>(object, "alignItems")? {
        style.align_items = Some(parse_item_alignment("grid", "alignItems", &value)?);
    }
    if let Some(value) = property::<String>(object, "justifyItems")? {
        style.justify_items = Some(parse_item_alignment("grid", "justifyItems", &value)?);
    }
    if let Some(value) = property::<String>(object, "gridRow")? {
        style.row = parse_grid_line("gridRow", &value)?;
    }
    if let Some(value) = property::<String>(object, "gridColumn")? {
        style.column = parse_grid_line("gridColumn", &value)?;
    }
    if let Some(value) = property::<String>(object, "alignSelf")? {
        style.align_self = Some(parse_item_alignment("grid", "alignSelf", &value)?);
    }
    if let Some(value) = property::<String>(object, "justifySelf")? {
        style.justify_self = Some(parse_item_alignment("grid", "justifySelf", &value)?);
    }

    apply_box_paint(
        "grid",
        object,
        &mut style.background,
        &mut style.border,
        &mut style.corner_radius,
    )?;
    Ok(style)
}

fn text_style_from_js(element: &Object<'_>, inherited: Option<&TextStyle>) -> Result<TextStyle> {
    let object = style_object(element)?;
    let object = object.as_ref();
    let mut style = inherited.cloned().unwrap_or_default();

    if let Some(value) = property::<String>(object, "color")? {
        style.color = parse_color("text", "color", &value)?;
    }
    if let Some(value) = property::<f32>(object, "fontSize")? {
        style.font_size = positive("text", "fontSize", value)?;
    }
    if let Some(value) = property::<Value<'_>>(object, "lineHeight")? {
        style.line_height = if let Some(number) = value.as_number() {
            LineHeight::Value(positive("text", "lineHeight", number as f32)?)
        } else if value.is_string() {
            let keyword = value.get::<String>()?;
            if keyword == "normal" {
                LineHeight::Normal
            } else {
                return Err(invalid(
                    "text",
                    "lineHeight",
                    quoted(&keyword),
                    "expected a positive number or \"normal\"",
                ));
            }
        } else {
            return Err(invalid(
                "text",
                "lineHeight",
                value.type_of().as_str(),
                "expected a positive number or \"normal\"",
            ));
        };
    }
    if let Some(value) = property::<Value<'_>>(object, "fontWeight")? {
        style.font_weight = if let Some(number) = value.as_number() {
            let number = number as f32;
            if number.is_finite() && number.fract() == 0.0 && (1.0..=1000.0).contains(&number) {
                number as u16
            } else {
                return Err(invalid(
                    "text",
                    "fontWeight",
                    number,
                    "expected an integer from 1 through 1000",
                ));
            }
        } else if value.is_string() {
            let keyword = value.get::<String>()?;
            match keyword.as_str() {
                "normal" => 400,
                "bold" => 700,
                _ => {
                    return Err(invalid(
                        "text",
                        "fontWeight",
                        quoted(&keyword),
                        "expected an integer, \"normal\", or \"bold\"",
                    ));
                }
            }
        } else {
            return Err(invalid(
                "text",
                "fontWeight",
                value.type_of().as_str(),
                "expected an integer, \"normal\", or \"bold\"",
            ));
        };
    }
    if let Some(value) = property::<String>(object, "fontFamily")? {
        style.font_families = parse_font_families(&value)?;
    }
    if let Some(value) = property::<String>(object, "fontStyle")? {
        style.font_style = match value.as_str() {
            "normal" => FontStyle::Normal,
            "italic" => FontStyle::Italic,
            "oblique" => FontStyle::Oblique,
            _ => {
                return Err(invalid(
                    "text",
                    "fontStyle",
                    quoted(&value),
                    "expected normal, italic, or oblique",
                ));
            }
        };
    }
    if let Some(value) = property::<String>(object, "textAlign")? {
        style.text_align = match value.as_str() {
            "start" => TextAlign::Start,
            "end" => TextAlign::End,
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            "center" => TextAlign::Center,
            "justify" => TextAlign::Justify,
            _ => {
                return Err(invalid(
                    "text",
                    "textAlign",
                    quoted(&value),
                    "expected start, end, left, right, center, or justify",
                ));
            }
        };
    }
    if let Some(value) = property::<f32>(object, "letterSpacing")? {
        style.letter_spacing = finite("text", "letterSpacing", value)?;
    }
    if let Some(value) = property::<f32>(object, "wordSpacing")? {
        style.word_spacing = finite("text", "wordSpacing", value)?;
    }
    if let Some(value) = property::<String>(object, "textDecorationLine")? {
        style.text_decoration_line = parse_text_decoration_line(&value)?;
    }
    if let Some(value) = property::<String>(object, "textDecorationColor")? {
        style.text_decoration_color =
            TextDecorationColor::Color(parse_color("text", "textDecorationColor", &value)?);
    }
    if let Some(value) = property::<String>(object, "whiteSpace")? {
        style.white_space = match value.as_str() {
            "normal" => TextWhiteSpace::Normal,
            "nowrap" => TextWhiteSpace::NoWrap,
            "pre" => TextWhiteSpace::Pre,
            "pre-wrap" => TextWhiteSpace::PreWrap,
            "pre-line" => TextWhiteSpace::PreLine,
            "break-spaces" => TextWhiteSpace::BreakSpaces,
            _ => {
                return Err(invalid(
                    "text",
                    "whiteSpace",
                    quoted(&value),
                    "expected normal, nowrap, pre, pre-wrap, pre-line, or break-spaces",
                ));
            }
        };
    }
    if let Some(value) = property::<String>(object, "overflowWrap")? {
        style.overflow_wrap = match value.as_str() {
            "normal" => TextOverflowWrap::Normal,
            "break-word" => TextOverflowWrap::BreakWord,
            "anywhere" => TextOverflowWrap::Anywhere,
            _ => {
                return Err(invalid(
                    "text",
                    "overflowWrap",
                    quoted(&value),
                    "expected normal, break-word, or anywhere",
                ));
            }
        };
    }
    if let Some(value) = property::<String>(object, "wordBreak")? {
        style.word_break = match value.as_str() {
            "normal" => TextWordBreak::Normal,
            "break-all" => TextWordBreak::BreakAll,
            "keep-all" => TextWordBreak::KeepAll,
            _ => {
                return Err(invalid(
                    "text",
                    "wordBreak",
                    quoted(&value),
                    "expected normal, break-all, or keep-all",
                ));
            }
        };
    }
    style.wrap = resolve_text_wrap(&style);
    Ok(style)
}

fn apply_box_paint<'js>(
    element: &'static str,
    style: Option<&Object<'js>>,
    background: &mut Background,
    border: &mut Option<Border>,
    corner_radius: &mut CornerRadius,
) -> Result<()> {
    if let Some(value) = property::<String>(style, "backgroundColor")? {
        background.color = parse_color(element, "backgroundColor", &value)?;
    }
    if let Some(value) = property::<Object<'js>>(style, "backgroundImage")? {
        background.image = Some(background_image_from_js(element, &value)?);
    }

    let border_color = property::<String>(style, "borderColor")?;
    let border_width = property::<f32>(style, "borderWidth")?;
    if border_color.is_some() || border_width.is_some() {
        *border = Some(Border::new(
            non_negative(element, "borderWidth", border_width.unwrap_or(0.0))?,
            border_color
                .as_deref()
                .map(|value| parse_color(element, "borderColor", value))
                .transpose()?
                .unwrap_or(Color::BLACK),
        ));
    }

    if let Some(value) = property::<f32>(style, "borderRadius")? {
        *corner_radius = CornerRadius::all(non_negative(element, "borderRadius", value)?);
    }
    Ok(())
}

fn background_image_from_js(element: &'static str, image: &Object<'_>) -> Result<BackgroundImage> {
    let image_type: String = image.get("type")?;
    match image_type.as_str() {
        "linear-gradient" => {
            let direction: Array<'_> = image.get("direction")?;
            if direction.len() != 2 {
                return Err(invalid(
                    element,
                    "backgroundImage.direction",
                    format!("array of length {}", direction.len()),
                    "expected exactly two numbers",
                ));
            }
            let x = finite(element, "backgroundImage.direction[0]", direction.get(0)?)?;
            let y = finite(element, "backgroundImage.direction[1]", direction.get(1)?)?;
            if x == 0.0 && y == 0.0 {
                return Err(invalid(
                    element,
                    "backgroundImage.direction",
                    "[0, 0]",
                    "the gradient direction cannot be the zero vector",
                ));
            }
            Ok(BackgroundImage::LinearGradient {
                direction: [x, y],
                stops: gradient_stops_from_js(element, image)?,
            })
        }
        "radial-gradient" => Ok(BackgroundImage::RadialGradient {
            stops: gradient_stops_from_js(element, image)?,
        }),
        "raster" => {
            let width: u32 = image.get("width")?;
            let height: u32 = image.get("height")?;
            let pixels: Array<'_> = image.get("pixels")?;
            let pixels = pixels.iter::<u8>().collect::<Result<Vec<_>>>()?;
            let byte_count = pixels.len();
            let image = RasterImage::new(width, height, pixels).ok_or_else(|| {
                invalid(
                    element,
                    "backgroundImage",
                    format!("raster {width}x{height} with {byte_count} bytes"),
                    "expected non-zero dimensions and exactly width * height * 4 RGBA bytes",
                )
            })?;
            Ok(BackgroundImage::Raster(image))
        }
        _ => Err(invalid(
            element,
            "backgroundImage.type",
            quoted(&image_type),
            "expected linear-gradient, radial-gradient, or raster",
        )),
    }
}

fn gradient_stops_from_js(element: &'static str, image: &Object<'_>) -> Result<Vec<GradientStop>> {
    let stops: Array<'_> = image.get("stops")?;
    if stops.is_empty() {
        return Err(invalid(
            element,
            "backgroundImage.stops",
            "[]",
            "expected at least one gradient stop",
        ));
    }

    let mut result = Vec::with_capacity(stops.len());
    let mut previous = 0.0;
    for (index, stop) in stops.iter::<Object<'_>>().enumerate() {
        let stop = stop?;
        let position = finite(
            element,
            &format!("backgroundImage.stops[{index}].position"),
            stop.get("position")?,
        )?;
        if !(0.0..=1.0).contains(&position) || (index > 0 && position < previous) {
            return Err(invalid(
                element,
                &format!("backgroundImage.stops[{index}].position"),
                position,
                "expected positions from 0 through 1 in ascending order",
            ));
        }
        let color: String = stop.get("color")?;
        result.push(GradientStop {
            color: parse_color(
                element,
                &format!("backgroundImage.stops[{index}].color"),
                &color,
            )?,
            position,
        });
        previous = position;
    }
    Ok(result)
}

fn apply_gap(
    element: &'static str,
    target: &mut Size<f32>,
    gap: Option<f32>,
    row_gap: Option<f32>,
    column_gap: Option<f32>,
) -> Result<()> {
    if let Some(value) = gap {
        let value = non_negative(element, "gap", value)?;
        *target = Size {
            width: value,
            height: value,
        };
    }
    if let Some(value) = row_gap {
        target.height = non_negative(element, "rowGap", value)?;
    }
    if let Some(value) = column_gap {
        target.width = non_negative(element, "columnGap", value)?;
    }
    Ok(())
}

type TemplateTracks = GridTemplateTracks<String, GridTemplateComponent<String>>;

fn parse_grid_template(property: &'static str, value: &str) -> Result<GridTemplate> {
    if value == "none" {
        return Ok(GridTemplate::default());
    }
    parse_css::<TemplateTracks>("grid", property, value).map(GridTemplate::from_taffy)
}

fn parse_grid_auto_tracks(property: &'static str, value: &str) -> Result<Vec<GridTrackSizing>> {
    parse_css::<GridAutoTracks>("grid", property, value).map(|tracks| {
        tracks
            .0
            .into_iter()
            .map(GridTrackSizing::from_taffy)
            .collect()
    })
}

fn parse_grid_line(property: &'static str, value: &str) -> Result<Line<GridPlacement<String>>> {
    let parts = value.split('/').map(str::trim).collect::<Vec<_>>();
    match parts.as_slice() {
        [start] if !start.is_empty() => Ok(Line {
            start: parse_css("grid", property, start)?,
            end: GridPlacement::Auto,
        }),
        [start, end] if !start.is_empty() && !end.is_empty() => Ok(Line {
            start: parse_css("grid", property, start)?,
            end: parse_css("grid", property, end)?,
        }),
        _ => Err(invalid(
            "grid",
            property,
            quoted(value),
            "expected one grid line or two grid lines separated by `/`",
        )),
    }
}

fn parse_font_families(value: &str) -> Result<Vec<FontFamily>> {
    let mut families = Vec::new();
    for family in value.split(',') {
        let family = unquote(family.trim());
        if family.is_empty() {
            return Err(invalid(
                "text",
                "fontFamily",
                quoted(value),
                "font family names cannot be empty",
            ));
        }
        families.push(match family.to_ascii_lowercase().as_str() {
            "sans-serif" => FontFamily::SansSerif,
            "serif" => FontFamily::Serif,
            "monospace" => FontFamily::Monospace,
            "cursive" => FontFamily::Cursive,
            "fantasy" => FontFamily::Fantasy,
            _ => FontFamily::Named(family.to_owned()),
        });
    }
    Ok(families)
}

fn unquote(value: &str) -> &str {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn parse_text_decoration_line(value: &str) -> Result<TextDecorationLine> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    if parts == ["none"] {
        return Ok(TextDecorationLine::NONE);
    }
    if parts.is_empty() || parts.contains(&"none") {
        return Err(invalid(
            "text",
            "textDecorationLine",
            quoted(value),
            "expected none or a combination of underline, overline, and line-through",
        ));
    }
    let mut line = TextDecorationLine::NONE;
    for part in parts {
        line = line.union(match part {
            "underline" => TextDecorationLine::UNDERLINE,
            "overline" => TextDecorationLine::OVERLINE,
            "line-through" => TextDecorationLine::LINE_THROUGH,
            _ => {
                return Err(invalid(
                    "text",
                    "textDecorationLine",
                    quoted(value),
                    "expected none or a combination of underline, overline, and line-through",
                ));
            }
        });
    }
    Ok(line)
}

fn resolve_text_wrap(style: &TextStyle) -> TextWrap {
    if matches!(
        style.white_space,
        TextWhiteSpace::NoWrap | TextWhiteSpace::Pre
    ) {
        return TextWrap::None;
    }
    if style.white_space == TextWhiteSpace::BreakSpaces {
        return TextWrap::Glyph;
    }
    match style.word_break {
        TextWordBreak::BreakAll => TextWrap::Glyph,
        TextWordBreak::KeepAll => TextWrap::Word,
        TextWordBreak::Normal => match style.overflow_wrap {
            TextOverflowWrap::Normal => TextWrap::Word,
            TextOverflowWrap::BreakWord => TextWrap::WordOrGlyph,
            TextOverflowWrap::Anywhere => TextWrap::Glyph,
        },
    }
}

fn parse_color(element: &'static str, property: &str, value: &str) -> Result<Color> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(invalid_hex_color(element, property, value));
    };
    let bytes = match hex.len() {
        3 | 4 => {
            let mut channels = [255; 4];
            for (index, digit) in hex.bytes().enumerate() {
                channels[index] = hex_digit(digit)
                    .map(|digit| digit * 17)
                    .ok_or_else(|| invalid_hex_color(element, property, value))?;
            }
            channels
        }
        6 | 8 => {
            let mut channels = [255; 4];
            for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
                let high = hex_digit(pair[0])
                    .ok_or_else(|| invalid_hex_color(element, property, value))?;
                let low = hex_digit(pair[1])
                    .ok_or_else(|| invalid_hex_color(element, property, value))?;
                channels[index] = high * 16 + low;
            }
            channels
        }
        _ => return Err(invalid_hex_color(element, property, value)),
    };
    Ok(Color::from_rgba8(bytes[0], bytes[1], bytes[2], bytes[3]))
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn invalid_hex_color(element: &'static str, property: &str, value: &str) -> Error {
    invalid(
        element,
        property,
        quoted(value),
        "expected #rgb, #rgba, #rrggbb, or #rrggbbaa",
    )
}

fn parse_content_alignment(
    element: &'static str,
    property: &str,
    value: &str,
) -> Result<AlignContent> {
    if !matches!(
        value,
        "start"
            | "end"
            | "flex-start"
            | "flex-end"
            | "center"
            | "stretch"
            | "space-between"
            | "space-around"
            | "space-evenly"
    ) {
        return Err(invalid(
            element,
            property,
            quoted(value),
            "unsupported content alignment",
        ));
    }
    parse_css(element, property, value)
}

fn parse_item_alignment(element: &'static str, property: &str, value: &str) -> Result<AlignItems> {
    if !matches!(
        value,
        "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch"
    ) {
        return Err(invalid(
            element,
            property,
            quoted(value),
            "unsupported item alignment",
        ));
    }
    parse_css(element, property, value)
}

fn parse_css<T>(element: &'static str, property: &str, value: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    value.parse().map_err(|error| {
        invalid(
            element,
            property,
            quoted(value),
            format!("unsupported CSS value ({error})"),
        )
    })
}

fn finite(element: &'static str, property: &str, value: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(
            element,
            property,
            value,
            "expected a finite number",
        ))
    }
}

fn non_negative(element: &'static str, property: &str, value: f32) -> Result<f32> {
    let value = finite(element, property, value)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(invalid(
            element,
            property,
            value,
            "expected a non-negative number",
        ))
    }
}

fn positive(element: &'static str, property: &str, value: f32) -> Result<f32> {
    let value = finite(element, property, value)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(invalid(
            element,
            property,
            value,
            "expected a positive number",
        ))
    }
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

fn invalid(
    element: &'static str,
    property: &str,
    value: impl ToString,
    reason: impl std::fmt::Display,
) -> Error {
    decode_error(format!(
        "invalid {element} style `{property}` value {}: {reason}",
        value.to_string()
    ))
}

fn decode_error(message: impl Into<String>) -> Error {
    Error::new_from_js_message("JavaScript element tree", "Elements", message.into())
}

#[cfg(test)]
mod tests {
    use render::{BackgroundImage, FontFamily, TextDecorationLine};
    use runtime::Runtime;
    use taffy::{FlexDirection, GridAutoFlow};

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn constructs_final_elements_directly_from_javascript() {
        let runtime = Runtime::new().await.unwrap();
        let tree: Elements = runtime
            .eval(
                r##"({
                  type: "app",
                  children: [{
                    type: "window",
                    children: [{
                      type: "flex",
                      style: {
                        flexDirection: "column",
                        gap: 12,
                        backgroundColor: "#102030",
                        unsupportedStyle: "ignored"
                      },
                      children: [{
                        type: "text",
                        style: {
                          color: "#abcdef",
                          fontSize: 20,
                          fontWeight: "bold",
                          fontFamily: "Inter, sans-serif",
                          textDecorationLine: "underline overline"
                        },
                        children: [
                          { type: "string", value: "hello " },
                          {
                            type: "text",
                            style: { fontWeight: 400 },
                            children: [{ type: "string", value: "world" }]
                          }
                        ]
                      }]
                    }]
                  }]
                })"##,
            )
            .await
            .unwrap();

        let Elements::App { children } = tree else {
            panic!("expected app");
        };
        let Elements::Window { children } = &children[0] else {
            panic!("expected window");
        };
        let Elements::Flex { style, children } = &children[0] else {
            panic!("expected flex");
        };
        assert_eq!(style.direction, FlexDirection::Column);
        assert_eq!(style.gap.width, 12.0);
        let Elements::Text { style, children } = &children[0] else {
            panic!("expected text");
        };
        assert_eq!(style.font_weight, 700);
        assert_eq!(style.font_families[0], FontFamily::Named("Inter".into()));
        assert_eq!(
            style.text_decoration_line,
            TextDecorationLine::UNDERLINE.union(TextDecorationLine::OVERLINE)
        );
        let Elements::Text { style, .. } = &children[1] else {
            panic!("expected nested text");
        };
        assert_eq!(style.font_weight, 400);
        assert_eq!(style.color, Color::from_rgba8(0xab, 0xcd, 0xef, 0xff));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn constructs_grid_and_background_images() {
        let runtime = Runtime::new().await.unwrap();
        let tree: Elements = runtime
            .eval(
                r##"({
                  type: "grid",
                  style: {
                    gridTemplateColumns: "[start] 20px 1fr [end]",
                    gridAutoRows: "minmax(10px, auto)",
                    gridAutoFlow: "row-dense",
                    gridColumn: "start / end",
                    backgroundImage: {
                      type: "linear-gradient",
                      direction: [1, 0],
                      stops: [
                        { color: "#000", position: 0 },
                        { color: "#fff", position: 1 }
                      ]
                    }
                  },
                  children: []
                })"##,
            )
            .await
            .unwrap();

        let Elements::Grid { style, .. } = tree else {
            panic!("expected grid");
        };
        assert_eq!(style.auto_flow, GridAutoFlow::RowDense);
        assert_eq!(style.template_columns.tracks.len(), 2);
        assert_eq!(style.template_columns.line_names[0], ["start"]);
        assert!(matches!(
            style.background.image,
            Some(BackgroundImage::LinearGradient { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn malformed_supported_values_are_rejected() {
        let runtime = Runtime::new().await.unwrap();
        let result = runtime
            .eval::<Elements>(
                r#"({ type: "flex", style: { flexDirection: "diagonal" }, children: [] })"#,
            )
            .await;
        assert!(result.is_err());
    }
}
