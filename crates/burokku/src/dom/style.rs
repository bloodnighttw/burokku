use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Block,
    Flex,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    Column,
}

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub display: Option<Display>,
    pub flex_direction: Option<FlexDirection>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub gap: Option<f32>,
    pub padding: Option<f32>,
    pub margin: Option<f32>,
    pub background_color: Option<[u8; 4]>,
    pub color: Option<[u8; 4]>,
    pub border_color: Option<[u8; 4]>,
    pub border_width: Option<f32>,
    pub border_radius: Option<f32>,
    pub outline_color: Option<[u8; 4]>,
    pub outline_width: Option<f32>,
    pub outline_offset: Option<f32>,
    pub font_size: Option<f32>,
    pub line_height: Option<f32>,
    pub font_weight: Option<u16>,
    pub font_family: Option<String>,
}

pub fn set_style(style: &mut Style, name: &str, value: Option<&str>) -> Result<(), StyleError> {
    let name = normalized_name(name);
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return clear_style(style, name);
    };

    macro_rules! length {
        ($field:ident) => {{
            style.$field = Some(parse_length(name, value)?);
            return Ok(());
        }};
    }
    macro_rules! color {
        ($field:ident) => {{
            style.$field = Some(parse_color(name, value)?);
            return Ok(());
        }};
    }

    match name {
        "display" => {
            style.display = Some(match value {
                "block" => Display::Block,
                "flex" | "inline-flex" => Display::Flex,
                "none" => Display::None,
                _ => return Err(StyleError::InvalidValue(name.into(), value.into())),
            });
        }
        "flex-direction" => {
            style.flex_direction = Some(match value {
                "row" => FlexDirection::Row,
                "column" => FlexDirection::Column,
                _ => return Err(StyleError::InvalidValue(name.into(), value.into())),
            });
        }
        "width" => length!(width),
        "height" => length!(height),
        "min-width" => length!(min_width),
        "min-height" => length!(min_height),
        "max-width" => length!(max_width),
        "max-height" => length!(max_height),
        "flex-grow" => length!(flex_grow),
        "flex-shrink" => length!(flex_shrink),
        "gap" => length!(gap),
        "padding" => length!(padding),
        "margin" => length!(margin),
        "border-width" => length!(border_width),
        "border-radius" => length!(border_radius),
        "outline-width" => length!(outline_width),
        "outline-offset" => length!(outline_offset),
        "font-size" => length!(font_size),
        "line-height" => length!(line_height),
        "background-color" => color!(background_color),
        "color" => color!(color),
        "border-color" => color!(border_color),
        "outline-color" => color!(outline_color),
        "font-weight" => {
            style.font_weight = Some(match value {
                "normal" => 400,
                "bold" => 700,
                _ => value
                    .parse::<u16>()
                    .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))?,
            });
        }
        "font-family" => style.font_family = Some(value.trim_matches(['\'', '"']).into()),
        _ => return Err(StyleError::UnsupportedProperty(name.into())),
    }
    Ok(())
}

fn clear_style(style: &mut Style, name: &str) -> Result<(), StyleError> {
    macro_rules! clear {
        ($field:ident) => {{
            style.$field = None;
            return Ok(());
        }};
    }
    match name {
        "display" => clear!(display),
        "flex-direction" => clear!(flex_direction),
        "width" => clear!(width),
        "height" => clear!(height),
        "min-width" => clear!(min_width),
        "min-height" => clear!(min_height),
        "max-width" => clear!(max_width),
        "max-height" => clear!(max_height),
        "flex-grow" => clear!(flex_grow),
        "flex-shrink" => clear!(flex_shrink),
        "gap" => clear!(gap),
        "padding" => clear!(padding),
        "margin" => clear!(margin),
        "background-color" => clear!(background_color),
        "color" => clear!(color),
        "border-color" => clear!(border_color),
        "border-width" => clear!(border_width),
        "border-radius" => clear!(border_radius),
        "outline-color" => clear!(outline_color),
        "outline-width" => clear!(outline_width),
        "outline-offset" => clear!(outline_offset),
        "font-size" => clear!(font_size),
        "line-height" => clear!(line_height),
        "font-weight" => clear!(font_weight),
        "font-family" => clear!(font_family),
        _ => Err(StyleError::UnsupportedProperty(name.into())),
    }
}

fn normalized_name(name: &str) -> &str {
    match name {
        "flexDirection" => "flex-direction",
        "minWidth" => "min-width",
        "minHeight" => "min-height",
        "maxWidth" => "max-width",
        "maxHeight" => "max-height",
        "flexGrow" => "flex-grow",
        "flexShrink" => "flex-shrink",
        "backgroundColor" => "background-color",
        "borderColor" => "border-color",
        "borderWidth" => "border-width",
        "borderRadius" => "border-radius",
        "outlineColor" => "outline-color",
        "outlineWidth" => "outline-width",
        "outlineOffset" => "outline-offset",
        "fontSize" => "font-size",
        "lineHeight" => "line-height",
        "fontWeight" => "font-weight",
        "fontFamily" => "font-family",
        other => other,
    }
}

fn parse_length(name: &str, value: &str) -> Result<f32, StyleError> {
    let value = value.strip_suffix("px").unwrap_or(value).trim();
    let parsed = value
        .parse::<f32>()
        .map_err(|_| StyleError::InvalidValue(name.into(), value.into()))?;
    if !parsed.is_finite() {
        return Err(StyleError::InvalidValue(name.into(), value.into()));
    }
    Ok(parsed)
}

fn parse_color(name: &str, value: &str) -> Result<[u8; 4], StyleError> {
    let named = match value.to_ascii_lowercase().as_str() {
        "transparent" => Some([0, 0, 0, 0]),
        "black" => Some([0, 0, 0, 255]),
        "white" => Some([255, 255, 255, 255]),
        "red" => Some([255, 0, 0, 255]),
        "green" => Some([0, 128, 0, 255]),
        "blue" => Some([0, 0, 255, 255]),
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
        _ => Err(StyleError::InvalidValue(name.into(), value.into())),
    }
}

#[derive(Debug, Error)]
pub enum StyleError {
    #[error("unsupported style property '{0}'")]
    UnsupportedProperty(String),
    #[error("invalid value '{1}' for style property '{0}'")]
    InvalidValue(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_dom_and_css_property_names() {
        let mut style = Style::default();
        set_style(&mut style, "backgroundColor", Some("#1234")).unwrap();
        set_style(&mut style, "font-size", Some("18px")).unwrap();
        assert_eq!(style.background_color, Some([17, 34, 51, 68]));
        assert_eq!(style.font_size, Some(18.0));

        set_style(&mut style, "font-size", None).unwrap();
        assert_eq!(style.font_size, None);
    }
}
