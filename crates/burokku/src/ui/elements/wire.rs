use std::str::FromStr;

use render::{
    BackgroundImage, Color, FontFamily, FontStyle, GradientStop, RasterImage, TextAlign,
    TextDecorationLine, TextOverflowWrap, TextWhiteSpace, TextWordBreak, TextWrap,
};
use serde::Deserialize;
use taffy::{
    geometry::{Line, Size},
    style::{GridAutoTracks, GridTemplateTracks},
    AlignContent, AlignItems, Dimension, GridAutoFlow, GridPlacement, GridTemplateComponent,
    LengthPercentage, TrackSizingFunction,
};
use thiserror::Error;

use super::{
    styles::{
        div::DivStyle,
        flex::FlexStyle,
        grid::GridStyle,
        shared::{background::Background, border::Border, corner_radius::CornerRadius},
        text::{LineHeight, TextDecorationColor, TextStyle},
    },
    Elements,
};

/// An error produced while decoding an [`Elements`] JSON tree.
#[derive(Debug, Error)]
pub enum ElementJsonError {
    #[error("invalid element JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid {element} style `{property}` value {value}: {reason}")]
    InvalidStyle {
        element: &'static str,
        property: String,
        value: String,
        reason: String,
    },
}

pub(super) fn parse(json: &str) -> Result<Elements, ElementJsonError> {
    serde_json::from_str::<WireElement>(json)?.into_element(None)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum WireElement {
    App {
        #[serde(default)]
        children: Vec<WireElement>,
    },
    Window {
        #[serde(default)]
        children: Vec<WireElement>,
    },
    Div {
        style: Option<WireDivStyle>,
        #[serde(default)]
        children: Vec<WireElement>,
    },
    Flex {
        style: Option<WireFlexStyle>,
        #[serde(default)]
        children: Vec<WireElement>,
    },
    Grid {
        style: Option<WireGridStyle>,
        #[serde(default)]
        children: Vec<WireElement>,
    },
    Text {
        style: Option<WireTextStyle>,
        #[serde(default)]
        children: Vec<WireElement>,
    },
    String {
        value: String,
    },
}

impl WireElement {
    fn into_element(
        self,
        inherited_text_style: Option<&TextStyle>,
    ) -> Result<Elements, ElementJsonError> {
        match self {
            Self::App { children } => Ok(Elements::App {
                children: convert_children(children, None)?,
            }),
            Self::Window { children } => Ok(Elements::Window {
                children: convert_children(children, None)?,
            }),
            Self::Div { style, children } => Ok(Elements::Div {
                style: Box::new(style.unwrap_or_default().into_style()?),
                children: convert_children(children, None)?,
            }),
            Self::Flex { style, children } => Ok(Elements::Flex {
                style: Box::new(style.unwrap_or_default().into_style()?),
                children: convert_children(children, None)?,
            }),
            Self::Grid { style, children } => Ok(Elements::Grid {
                style: Box::new(style.unwrap_or_default().into_style()?),
                children: convert_children(children, None)?,
            }),
            Self::Text { style, children } => {
                let style = Box::new(style.unwrap_or_default().into_style(inherited_text_style)?);
                let children = convert_children(children, Some(&style))?;
                Ok(Elements::Text { style, children })
            }
            Self::String { value } => Ok(Elements::_String { string: value }),
        }
    }
}

fn convert_children(
    children: Vec<WireElement>,
    inherited_text_style: Option<&TextStyle>,
) -> Result<Vec<Elements>, ElementJsonError> {
    children
        .into_iter()
        .map(|child| child.into_element(inherited_text_style))
        .collect()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireDivStyle {
    background_color: Option<String>,
    background_image: Option<WireBackgroundImage>,
    border_color: Option<String>,
    border_width: Option<f32>,
    border_radius: Option<WireCornerRadius>,
}

impl WireDivStyle {
    fn into_style(self) -> Result<DivStyle, ElementJsonError> {
        let paint = box_paint(
            "div",
            self.background_color,
            self.background_image,
            self.border_color,
            self.border_width,
            self.border_radius,
        )?;
        Ok(DivStyle {
            background: paint.background,
            border: paint.border,
            corner_radius: paint.corner_radius,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireFlexStyle {
    flex_direction: Option<String>,
    flex_wrap: Option<String>,
    gap: Option<f32>,
    row_gap: Option<f32>,
    column_gap: Option<f32>,
    align_content: Option<String>,
    align_items: Option<String>,
    justify_content: Option<String>,
    flex_basis: Option<WireNumberOrAuto>,
    flex_grow: Option<f32>,
    flex_shrink: Option<f32>,
    align_self: Option<String>,
    background_color: Option<String>,
    background_image: Option<WireBackgroundImage>,
    border_color: Option<String>,
    border_width: Option<f32>,
    border_radius: Option<WireCornerRadius>,
}

impl WireFlexStyle {
    fn into_style(self) -> Result<FlexStyle, ElementJsonError> {
        let mut style = FlexStyle::default();
        if let Some(value) = self.flex_direction {
            style.direction = parse_css("flex", "flexDirection", &value)?;
        }
        if let Some(value) = self.flex_wrap {
            style.wrap = parse_css("flex", "flexWrap", &value)?;
        }
        apply_gap(
            "flex",
            &mut style.gap,
            self.gap,
            self.row_gap,
            self.column_gap,
        )?;
        if let Some(value) = self.align_content {
            style.align_content = Some(parse_content_alignment("flex", "alignContent", &value)?);
        }
        if let Some(value) = self.align_items {
            style.align_items = Some(parse_item_alignment("flex", "alignItems", &value)?);
        }
        if let Some(value) = self.justify_content {
            style.justify_content =
                Some(parse_content_alignment("flex", "justifyContent", &value)?);
        }
        if let Some(value) = self.flex_basis {
            style.basis = match value {
                WireNumberOrAuto::Number(value) => {
                    Dimension::length(non_negative("flex", "flexBasis", value)?)
                }
                WireNumberOrAuto::Keyword(value) if value == "auto" => Dimension::auto(),
                WireNumberOrAuto::Keyword(value) => {
                    return Err(invalid(
                        "flex",
                        "flexBasis",
                        quoted(&value),
                        "expected a non-negative number or \"auto\"",
                    ));
                }
            };
        }
        if let Some(value) = self.flex_grow {
            style.grow = non_negative("flex", "flexGrow", value)?;
        }
        if let Some(value) = self.flex_shrink {
            style.shrink = non_negative("flex", "flexShrink", value)?;
        }
        if let Some(value) = self.align_self {
            style.align_self = Some(parse_item_alignment("flex", "alignSelf", &value)?);
        }

        let paint = box_paint(
            "flex",
            self.background_color,
            self.background_image,
            self.border_color,
            self.border_width,
            self.border_radius,
        )?;
        style.background = paint.background;
        style.border = paint.border;
        style.corner_radius = paint.corner_radius;
        Ok(style)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireGridStyle {
    grid_template_columns: Option<String>,
    grid_template_rows: Option<String>,
    grid_auto_columns: Option<String>,
    grid_auto_rows: Option<String>,
    grid_auto_flow: Option<String>,
    gap: Option<f32>,
    row_gap: Option<f32>,
    column_gap: Option<f32>,
    align_content: Option<String>,
    justify_content: Option<String>,
    align_items: Option<String>,
    justify_items: Option<String>,
    grid_row: Option<String>,
    grid_column: Option<String>,
    align_self: Option<String>,
    justify_self: Option<String>,
    background_color: Option<String>,
    background_image: Option<WireBackgroundImage>,
    border_color: Option<String>,
    border_width: Option<f32>,
    border_radius: Option<WireCornerRadius>,
}

impl WireGridStyle {
    fn into_style(self) -> Result<GridStyle, ElementJsonError> {
        let mut style = GridStyle::default();
        if let Some(value) = self.grid_template_columns {
            let tracks = parse_grid_template("gridTemplateColumns", &value)?;
            style.template_columns = tracks.tracks;
            style.template_column_names = tracks.line_names;
        }
        if let Some(value) = self.grid_template_rows {
            let tracks = parse_grid_template("gridTemplateRows", &value)?;
            style.template_rows = tracks.tracks;
            style.template_row_names = tracks.line_names;
        }
        if let Some(value) = self.grid_auto_columns {
            style.auto_columns = parse_grid_auto_tracks("gridAutoColumns", &value)?;
        }
        if let Some(value) = self.grid_auto_rows {
            style.auto_rows = parse_grid_auto_tracks("gridAutoRows", &value)?;
        }
        if let Some(value) = self.grid_auto_flow {
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
            self.gap,
            self.row_gap,
            self.column_gap,
        )?;
        if let Some(value) = self.align_content {
            style.align_content = Some(parse_content_alignment("grid", "alignContent", &value)?);
        }
        if let Some(value) = self.justify_content {
            style.justify_content =
                Some(parse_content_alignment("grid", "justifyContent", &value)?);
        }
        if let Some(value) = self.align_items {
            style.align_items = Some(parse_item_alignment("grid", "alignItems", &value)?);
        }
        if let Some(value) = self.justify_items {
            style.justify_items = Some(parse_item_alignment("grid", "justifyItems", &value)?);
        }
        if let Some(value) = self.grid_row {
            style.row = parse_grid_line("gridRow", &value)?;
        }
        if let Some(value) = self.grid_column {
            style.column = parse_grid_line("gridColumn", &value)?;
        }
        if let Some(value) = self.align_self {
            style.align_self = Some(parse_item_alignment("grid", "alignSelf", &value)?);
        }
        if let Some(value) = self.justify_self {
            style.justify_self = Some(parse_item_alignment("grid", "justifySelf", &value)?);
        }

        let paint = box_paint(
            "grid",
            self.background_color,
            self.background_image,
            self.border_color,
            self.border_width,
            self.border_radius,
        )?;
        style.background = paint.background;
        style.border = paint.border;
        style.corner_radius = paint.corner_radius;
        Ok(style)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireTextStyle {
    color: Option<String>,
    font_size: Option<f32>,
    line_height: Option<WireLineHeight>,
    font_weight: Option<WireFontWeight>,
    font_family: Option<WireFontFamily>,
    font_style: Option<String>,
    text_align: Option<String>,
    letter_spacing: Option<f32>,
    word_spacing: Option<f32>,
    text_decoration_line: Option<String>,
    text_decoration_color: Option<String>,
    white_space: Option<String>,
    overflow_wrap: Option<String>,
    word_break: Option<String>,
}

impl WireTextStyle {
    fn into_style(self, inherited: Option<&TextStyle>) -> Result<TextStyle, ElementJsonError> {
        let mut style = inherited.cloned().unwrap_or_default();
        if let Some(value) = self.color {
            style.color = parse_color("text", "color", &value)?;
        }
        if let Some(value) = self.font_size {
            style.font_size = positive("text", "fontSize", value)?;
        }
        if let Some(value) = self.line_height {
            style.line_height = match value {
                WireLineHeight::Number(value) => {
                    LineHeight::Value(positive("text", "lineHeight", value)?)
                }
                WireLineHeight::Keyword(value) if value == "normal" => LineHeight::Normal,
                WireLineHeight::Keyword(value) => {
                    return Err(invalid(
                        "text",
                        "lineHeight",
                        quoted(&value),
                        "expected a positive number or \"normal\"",
                    ));
                }
            };
        }
        if let Some(value) = self.font_weight {
            style.font_weight = match value {
                WireFontWeight::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && (1.0..=1000.0).contains(&value) =>
                {
                    value as u16
                }
                WireFontWeight::Number(value) => {
                    return Err(invalid(
                        "text",
                        "fontWeight",
                        value,
                        "expected an integer from 1 through 1000",
                    ));
                }
                WireFontWeight::Keyword(value) if value == "normal" => 400,
                WireFontWeight::Keyword(value) if value == "bold" => 700,
                WireFontWeight::Keyword(value) => {
                    return Err(invalid(
                        "text",
                        "fontWeight",
                        quoted(&value),
                        "expected an integer, \"normal\", or \"bold\"",
                    ));
                }
            };
        }
        if let Some(value) = self.font_family {
            style.font_families = parse_font_families(value)?;
        }
        if let Some(value) = self.font_style {
            style.font_style = match value.as_str() {
                "normal" => FontStyle::Normal,
                "italic" => FontStyle::Italic,
                "oblique" => FontStyle::Oblique,
                _ => {
                    return Err(invalid(
                        "text",
                        "fontStyle",
                        quoted(&value),
                        "expected \"normal\", \"italic\", or \"oblique\"",
                    ));
                }
            };
        }
        if let Some(value) = self.text_align {
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
        if let Some(value) = self.letter_spacing {
            style.letter_spacing = finite("text", "letterSpacing", value)?;
        }
        if let Some(value) = self.word_spacing {
            style.word_spacing = finite("text", "wordSpacing", value)?;
        }
        if let Some(value) = self.text_decoration_line {
            style.text_decoration_line = parse_text_decoration_line(&value)?;
        }
        if let Some(value) = self.text_decoration_color {
            style.text_decoration_color =
                TextDecorationColor::Color(parse_color("text", "textDecorationColor", &value)?);
        }
        if let Some(value) = self.white_space {
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
        if let Some(value) = self.overflow_wrap {
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
        if let Some(value) = self.word_break {
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
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireNumberOrAuto {
    Number(f32),
    Keyword(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireLineHeight {
    Number(f32),
    Keyword(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireFontWeight {
    Number(f32),
    Keyword(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireFontFamily {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WireCornerRadius {
    All(f32),
    Corners(WireCornerRadii),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireCornerRadii {
    top_left: f32,
    top_right: f32,
    bottom_right: f32,
    bottom_left: f32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum WireBackgroundImage {
    LinearGradient {
        direction: [f32; 2],
        stops: Vec<WireGradientStop>,
    },
    RadialGradient {
        stops: Vec<WireGradientStop>,
    },
    Raster {
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireGradientStop {
    color: String,
    position: f32,
}

struct BoxPaint {
    background: Background,
    border: Option<Border>,
    corner_radius: CornerRadius,
}

fn box_paint(
    element: &'static str,
    background_color: Option<String>,
    background_image: Option<WireBackgroundImage>,
    border_color: Option<String>,
    border_width: Option<f32>,
    border_radius: Option<WireCornerRadius>,
) -> Result<BoxPaint, ElementJsonError> {
    let background = Background {
        color: match background_color {
            Some(value) => parse_color(element, "backgroundColor", &value)?,
            None => Color::TRANSPARENT,
        },
        image: background_image
            .map(|value| value.into_background(element))
            .transpose()?,
    };
    let border = if border_color.is_some() || border_width.is_some() {
        Some(Border::new(
            non_negative(element, "borderWidth", border_width.unwrap_or(0.0))?,
            match border_color {
                Some(value) => parse_color(element, "borderColor", &value)?,
                None => Color::BLACK,
            },
        ))
    } else {
        None
    };
    let corner_radius = match border_radius {
        None => CornerRadius::ZERO,
        Some(WireCornerRadius::All(value)) => {
            CornerRadius::all(non_negative(element, "borderRadius", value)?)
        }
        Some(WireCornerRadius::Corners(value)) => CornerRadius::new(
            non_negative(element, "borderRadius.topLeft", value.top_left)?,
            non_negative(element, "borderRadius.topRight", value.top_right)?,
            non_negative(element, "borderRadius.bottomRight", value.bottom_right)?,
            non_negative(element, "borderRadius.bottomLeft", value.bottom_left)?,
        ),
    };
    Ok(BoxPaint {
        background,
        border,
        corner_radius,
    })
}

impl WireBackgroundImage {
    fn into_background(self, element: &'static str) -> Result<BackgroundImage, ElementJsonError> {
        match self {
            Self::LinearGradient { direction, stops } => {
                let x = finite(element, "backgroundImage.direction[0]", direction[0])?;
                let y = finite(element, "backgroundImage.direction[1]", direction[1])?;
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
                    stops: gradient_stops(element, stops)?,
                })
            }
            Self::RadialGradient { stops } => Ok(BackgroundImage::RadialGradient {
                stops: gradient_stops(element, stops)?,
            }),
            Self::Raster {
                width,
                height,
                pixels,
            } => {
                let pixel_count = pixels.len();
                let image = RasterImage::new(width, height, pixels).ok_or_else(|| {
                    invalid(
                        element,
                        "backgroundImage",
                        format!("raster {width}x{height} with {pixel_count} bytes"),
                        "expected non-zero dimensions and exactly width * height * 4 RGBA bytes",
                    )
                })?;
                Ok(BackgroundImage::Raster(image))
            }
        }
    }
}

fn gradient_stops(
    element: &'static str,
    stops: Vec<WireGradientStop>,
) -> Result<Vec<GradientStop>, ElementJsonError> {
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
    for (index, stop) in stops.into_iter().enumerate() {
        let property = format!("backgroundImage.stops[{index}].position");
        let position = finite(element, &property, stop.position)?;
        if !(0.0..=1.0).contains(&position) || (index > 0 && position < previous) {
            return Err(invalid(
                element,
                &property,
                position,
                "expected positions from 0 through 1 in ascending order",
            ));
        }
        result.push(GradientStop {
            color: parse_color(
                element,
                &format!("backgroundImage.stops[{index}].color"),
                &stop.color,
            )?,
            position,
        });
        previous = position;
    }
    Ok(result)
}

fn apply_gap(
    element: &'static str,
    target: &mut Size<LengthPercentage>,
    gap: Option<f32>,
    row_gap: Option<f32>,
    column_gap: Option<f32>,
) -> Result<(), ElementJsonError> {
    if let Some(value) = gap {
        let value = LengthPercentage::length(non_negative(element, "gap", value)?);
        *target = Size {
            width: value,
            height: value,
        };
    }
    if let Some(value) = row_gap {
        target.height = LengthPercentage::length(non_negative(element, "rowGap", value)?);
    }
    if let Some(value) = column_gap {
        target.width = LengthPercentage::length(non_negative(element, "columnGap", value)?);
    }
    Ok(())
}

type TemplateTracks = GridTemplateTracks<String, GridTemplateComponent<String>>;

fn parse_grid_template(
    property: &'static str,
    value: &str,
) -> Result<TemplateTracks, ElementJsonError> {
    if value == "none" {
        return Ok(TemplateTracks::default());
    }
    parse_css("grid", property, value)
}

fn parse_grid_auto_tracks(
    property: &'static str,
    value: &str,
) -> Result<Vec<TrackSizingFunction>, ElementJsonError> {
    Ok(parse_css::<GridAutoTracks>("grid", property, value)?.0)
}

fn parse_grid_line(
    property: &'static str,
    value: &str,
) -> Result<Line<GridPlacement<String>>, ElementJsonError> {
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

fn parse_font_families(value: WireFontFamily) -> Result<Vec<FontFamily>, ElementJsonError> {
    let raw = match value {
        WireFontFamily::One(value) => vec![value],
        WireFontFamily::Many(values) => values,
    };
    let mut families = Vec::new();
    for value in raw {
        for family in value.split(',') {
            let family = unquote(family.trim());
            if family.is_empty() {
                return Err(invalid(
                    "text",
                    "fontFamily",
                    quoted(&value),
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
    }
    if families.is_empty() {
        return Err(invalid(
            "text",
            "fontFamily",
            "[]",
            "expected at least one font family",
        ));
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

fn parse_text_decoration_line(value: &str) -> Result<TextDecorationLine, ElementJsonError> {
    let parts = value.split_ascii_whitespace().collect::<Vec<_>>();
    if parts == ["none"] {
        return Ok(TextDecorationLine::NONE);
    }
    if parts.is_empty() || parts.contains(&"none") {
        return Err(invalid(
            "text",
            "textDecorationLine",
            quoted(value),
            "expected none or a space-separated combination of underline, overline, and line-through",
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
                    "expected none or a space-separated combination of underline, overline, and line-through",
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

fn parse_color(
    element: &'static str,
    property: &str,
    value: &str,
) -> Result<Color, ElementJsonError> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(invalid(
            element,
            property,
            quoted(value),
            "expected #rgb, #rgba, #rrggbb, or #rrggbbaa",
        ));
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

fn invalid_hex_color(element: &'static str, property: &str, value: &str) -> ElementJsonError {
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
) -> Result<AlignContent, ElementJsonError> {
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
            "expected start, end, flex-start, flex-end, center, stretch, space-between, space-around, or space-evenly",
        ));
    }
    parse_css(element, property, value)
}

fn parse_item_alignment(
    element: &'static str,
    property: &str,
    value: &str,
) -> Result<AlignItems, ElementJsonError> {
    if !matches!(
        value,
        "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch"
    ) {
        return Err(invalid(
            element,
            property,
            quoted(value),
            "expected start, end, flex-start, flex-end, center, baseline, or stretch",
        ));
    }
    parse_css(element, property, value)
}

fn parse_css<T>(element: &'static str, property: &str, value: &str) -> Result<T, ElementJsonError>
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

fn finite(element: &'static str, property: &str, value: f32) -> Result<f32, ElementJsonError> {
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

fn non_negative(
    element: &'static str,
    property: &str,
    value: f32,
) -> Result<f32, ElementJsonError> {
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

fn positive(element: &'static str, property: &str, value: f32) -> Result<f32, ElementJsonError> {
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
    reason: impl Into<String>,
) -> ElementJsonError {
    ElementJsonError::InvalidStyle {
        element,
        property: property.to_owned(),
        value: value.to_string(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use render::{BackgroundImage, FontFamily, TextDecorationLine};
    use taffy::{
        style_helpers::TaffyZero, AlignContent, FlexDirection, FlexWrap, GridAutoFlow,
        GridPlacement,
    };

    use super::*;

    #[test]
    fn parses_a_styled_element_tree() {
        let tree = Elements::from_json(
            r##"{
                "type":"app",
                "children":[{
                    "type":"window",
                    "children":[{
                        "type":"flex",
                        "style":{
                            "flexDirection":"column-reverse",
                            "flexWrap":"wrap-reverse",
                            "gap":4,
                            "rowGap":8,
                            "columnGap":6,
                            "alignContent":"space-between",
                            "alignItems":"center",
                            "justifyContent":"center",
                            "flexBasis":24,
                            "flexGrow":2,
                            "flexShrink":0,
                            "alignSelf":"stretch",
                            "backgroundColor":"#1234",
                            "borderColor":"#abcdef",
                            "borderWidth":2,
                            "borderRadius":{
                                "topLeft":1,
                                "topRight":2,
                                "bottomRight":3,
                                "bottomLeft":4
                            }
                        },
                        "children":[{
                            "type":"text",
                            "style":{"fontSize":20,"color":"#102030"},
                            "children":[{"type":"string","value":"hello"}]
                        }]
                    }]
                }]
            }"##,
        )
        .unwrap();

        let Elements::App { children } = tree else {
            panic!("expected app root");
        };
        let Elements::Window { children } = &children[0] else {
            panic!("expected window");
        };
        let Elements::Flex { style, children } = &children[0] else {
            panic!("expected flex");
        };
        assert_eq!(style.direction, FlexDirection::ColumnReverse);
        assert_eq!(style.wrap, FlexWrap::WrapReverse);
        assert_eq!(style.align_content, Some(AlignContent::SPACE_BETWEEN));
        assert_eq!(style.grow, 2.0);
        assert_eq!(style.shrink, 0.0);
        assert_eq!(style.corner_radius, CornerRadius::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(style.background.color, Color::from_rgba8(17, 34, 51, 68));
        assert_eq!(
            style.border,
            Some(Border::new(2.0, Color::from_rgba8(171, 205, 239, 255)))
        );

        let Elements::Text { style, children } = &children[0] else {
            panic!("expected text");
        };
        assert_eq!(style.font_size, 20.0);
        assert_eq!(style.color, Color::from_rgba8(16, 32, 48, 255));
        assert!(matches!(&children[0], Elements::_String { string } if string == "hello"));
    }

    #[test]
    fn parses_grid_tracks_auto_tracks_and_placement() {
        let tree = Elements::from_json(
            r#"{
                "type":"grid",
                "style":{
                    "gridTemplateColumns":"[left] 1fr 20px [right]",
                    "gridTemplateRows":"repeat(2, minmax(10px, auto))",
                    "gridAutoColumns":"40px 10%",
                    "gridAutoRows":"min-content 1fr",
                    "gridAutoFlow":"column-dense",
                    "gridRow":"2 / span 3",
                    "gridColumn":"content / -1"
                }
            }"#,
        )
        .unwrap();

        let Elements::Grid { style, .. } = tree else {
            panic!("expected grid");
        };
        assert_eq!(style.template_columns.len(), 2);
        assert_eq!(
            style
                .template_column_names
                .iter()
                .flatten()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["left", "right"]
        );
        assert_eq!(style.template_rows.len(), 1);
        assert_eq!(style.auto_columns.len(), 2);
        assert_eq!(style.auto_rows.len(), 2);
        assert_eq!(style.auto_flow, GridAutoFlow::ColumnDense);
        assert!(!matches!(style.row.start, GridPlacement::Auto));
        assert!(!matches!(style.row.end, GridPlacement::Auto));
        assert!(!matches!(style.column.start, GridPlacement::Auto));
        assert!(!matches!(style.column.end, GridPlacement::Auto));
    }

    #[test]
    fn nested_text_inherits_and_overrides_typography() {
        let tree = Elements::from_json(
            r##"{
                "type":"text",
                "style":{
                    "color":"#112233",
                    "fontSize":24,
                    "fontFamily":["Inter","sans-serif"],
                    "whiteSpace":"nowrap",
                    "textDecorationLine":"underline overline"
                },
                "children":[{
                    "type":"text",
                    "style":{"fontWeight":"bold","color":"#ffffff"},
                    "children":[{"type":"string","value":"nested"}]
                }]
            }"##,
        )
        .unwrap();

        let Elements::Text { style, children } = tree else {
            panic!("expected text");
        };
        assert_eq!(style.wrap, TextWrap::None);
        assert_eq!(
            style.font_families,
            [FontFamily::Named("Inter".into()), FontFamily::SansSerif]
        );
        assert!(style
            .text_decoration_line
            .contains(TextDecorationLine::UNDERLINE));
        assert!(style
            .text_decoration_line
            .contains(TextDecorationLine::OVERLINE));

        let Elements::Text { style: nested, .. } = &children[0] else {
            panic!("expected nested text");
        };
        assert_eq!(nested.font_size, 24.0);
        assert_eq!(nested.font_weight, 700);
        assert_eq!(nested.color, Color::WHITE);
        assert_eq!(nested.wrap, TextWrap::None);
    }

    #[test]
    fn parses_structured_background_images() {
        let gradient = Elements::from_json(
            r##"{
                "type":"div",
                "style":{"backgroundImage":{
                    "type":"linear-gradient",
                    "direction":[1,0],
                    "stops":[
                        {"color":"#000","position":0},
                        {"color":"#ffffff","position":1}
                    ]
                }}
            }"##,
        )
        .unwrap();
        let Elements::Div { style, .. } = gradient else {
            panic!("expected div");
        };
        assert!(matches!(
            style.background.image,
            Some(BackgroundImage::LinearGradient { ref stops, .. }) if stops.len() == 2
        ));

        let raster = Elements::from_json(
            r#"{
                "type":"div",
                "style":{"backgroundImage":{
                    "type":"raster",
                    "width":1,
                    "height":1,
                    "pixels":[255,0,0,255]
                }}
            }"#,
        )
        .unwrap();
        let Elements::Div { style, .. } = raster else {
            panic!("expected div");
        };
        assert!(matches!(
            style.background.image,
            Some(BackgroundImage::Raster(_))
        ));
    }

    #[test]
    fn rejects_unknown_style_keys_and_invalid_values() {
        let unknown = Elements::from_json(r#"{"type":"flex","style":{"width":100}}"#).unwrap_err();
        assert!(unknown.to_string().contains("unknown field `width`"));

        let color =
            Elements::from_json(r##"{"type":"text","style":{"color":"red"}}"##).unwrap_err();
        assert!(color.to_string().contains("text style `color`"));
        assert!(color.to_string().contains("#rrggbb"));

        let direction =
            Elements::from_json(r#"{"type":"flex","style":{"flexDirection":"diagonal"}}"#)
                .unwrap_err();
        assert!(direction.to_string().contains("flexDirection"));

        let alignment =
            Elements::from_json(r#"{"type":"grid","style":{"alignItems":"safe center"}}"#)
                .unwrap_err();
        assert!(alignment.to_string().contains("alignItems"));

        let raster = Elements::from_json(
            r#"{"type":"div","style":{"backgroundImage":{"type":"raster","width":2,"height":2,"pixels":[0,0,0,0]}}}"#,
        )
        .unwrap_err();
        assert!(raster.to_string().contains("width * height * 4"));
    }

    #[test]
    fn omitted_styles_use_native_defaults() {
        let div = Elements::from_json(r#"{"type":"div"}"#).unwrap();
        let Elements::Div { style, .. } = div else {
            panic!("expected div");
        };
        assert_eq!(*style, DivStyle::default());

        let grid = Elements::from_json(r#"{"type":"grid","style":{"gridTemplateColumns":"none"}}"#)
            .unwrap();
        let Elements::Grid { style, .. } = grid else {
            panic!("expected grid");
        };
        assert!(style.template_columns.is_empty());
        assert_eq!(style.gap.width, LengthPercentage::ZERO);
    }
}
