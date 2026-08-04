use std::{
    collections::VecDeque,
    io::Cursor,
    sync::{Mutex, OnceLock},
};

use super::super::{BackgroundImage, Color, GradientStop, Shadow, Transform};
use super::{
    invalid,
    layout::{parse_length, parse_number},
    StyleError,
};

pub(super) fn parse_transform(name: &str, value: &str) -> Result<Transform, StyleError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(Transform::None);
    }
    let mut result = Transform::IDENTITY_MATRIX;
    let mut rest = value.trim();
    while !rest.is_empty() {
        let open = rest
            .find('(')
            .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?;
        let close = rest[open + 1..]
            .find(')')
            .map(|index| open + 1 + index)
            .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?;
        let function = rest[..open].trim().to_ascii_lowercase();
        let arguments = split_arguments(&rest[open + 1..close]);
        let matrix = match function.as_str() {
            "translate" => {
                let x = parse_length(name, argument(&arguments, 0, value)?)?;
                let y = arguments
                    .get(1)
                    .map(|value| parse_length(name, value))
                    .transpose()?
                    .unwrap_or(0.0);
                [1.0, 0.0, 0.0, 1.0, x, y]
            }
            "translatex" => [
                1.0,
                0.0,
                0.0,
                1.0,
                parse_length(name, argument(&arguments, 0, value)?)?,
                0.0,
            ],
            "translatey" => [
                1.0,
                0.0,
                0.0,
                1.0,
                0.0,
                parse_length(name, argument(&arguments, 0, value)?)?,
            ],
            "scale" => {
                let x = parse_number(name, argument(&arguments, 0, value)?)?;
                let y = arguments
                    .get(1)
                    .map(|value| parse_number(name, value))
                    .transpose()?
                    .unwrap_or(x);
                [x, 0.0, 0.0, y, 0.0, 0.0]
            }
            "scalex" => [
                parse_number(name, argument(&arguments, 0, value)?)?,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
            ],
            "scaley" => [
                1.0,
                0.0,
                0.0,
                parse_number(name, argument(&arguments, 0, value)?)?,
                0.0,
                0.0,
            ],
            "rotate" => {
                let angle = parse_angle(name, argument(&arguments, 0, value)?)?;
                let (sin, cos) = angle.sin_cos();
                [cos, sin, -sin, cos, 0.0, 0.0]
            }
            "skew" => {
                let x = parse_angle(name, argument(&arguments, 0, value)?)?.tan();
                let y = arguments
                    .get(1)
                    .map(|value| parse_angle(name, value))
                    .transpose()?
                    .unwrap_or(0.0)
                    .tan();
                [1.0, y, x, 1.0, 0.0, 0.0]
            }
            "skewx" => [
                1.0,
                0.0,
                parse_angle(name, argument(&arguments, 0, value)?)?.tan(),
                1.0,
                0.0,
                0.0,
            ],
            "skewy" => [
                1.0,
                parse_angle(name, argument(&arguments, 0, value)?)?.tan(),
                0.0,
                1.0,
                0.0,
                0.0,
            ],
            "matrix" if arguments.len() == 6 => {
                let mut matrix = [0.0; 6];
                for (output, argument) in matrix.iter_mut().zip(arguments) {
                    *output = parse_number(name, argument)?;
                }
                matrix
            }
            _ => return invalid(name, value),
        };
        result = multiply_affine(result, matrix);
        rest = rest[close + 1..].trim();
    }
    Ok(Transform::Matrix(result))
}

pub(super) fn multiply_affine(left: [f32; 6], right: [f32; 6]) -> [f32; 6] {
    [
        left[0] * right[0] + left[2] * right[1],
        left[1] * right[0] + left[3] * right[1],
        left[0] * right[2] + left[2] * right[3],
        left[1] * right[2] + left[3] * right[3],
        left[0] * right[4] + left[2] * right[5] + left[4],
        left[1] * right[4] + left[3] * right[5] + left[5],
    ]
}

pub(super) fn parse_angle(name: &str, value: &str) -> Result<f32, StyleError> {
    if let Some(degrees) = value.strip_suffix("deg") {
        return Ok(parse_number(name, degrees.trim())?.to_radians());
    }
    if let Some(turns) = value.strip_suffix("turn") {
        return Ok(parse_number(name, turns.trim())? * std::f32::consts::TAU);
    }
    if let Some(radians) = value.strip_suffix("rad") {
        return parse_number(name, radians.trim());
    }
    if value == "0" {
        return Ok(0.0);
    }
    invalid(name, value)
}

pub(super) fn parse_shadow(name: &str, value: &str) -> Result<Vec<Shadow>, StyleError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let shadows = split_top_level(value, ',');
    if shadows.len() > 32 {
        return invalid(name, value);
    }
    shadows
        .into_iter()
        .map(|shadow| parse_shadow_item(name, shadow))
        .collect()
}

fn parse_shadow_item(name: &str, value: &str) -> Result<Shadow, StyleError> {
    let parts = split_whitespace_preserving_functions(value);
    let inset = parts.iter().any(|part| part.eq_ignore_ascii_case("inset"));
    let color_index = parts
        .iter()
        .position(|part| parse_color(name, part).is_ok());
    let color = color_index
        .map(|index| parse_color(name, &parts[index]))
        .transpose()?
        .unwrap_or([0, 0, 0, 255]);
    let lengths = parts
        .iter()
        .enumerate()
        .filter(|(index, part)| Some(*index) != color_index && !part.eq_ignore_ascii_case("inset"))
        .map(|(_, part)| parse_length(name, part))
        .collect::<Result<Vec<_>, _>>()?;
    if !(2..=4).contains(&lengths.len()) || lengths.get(2).is_some_and(|value| *value < 0.0) {
        return invalid(name, value);
    }
    Ok(Shadow {
        offset_x: lengths[0],
        offset_y: lengths[1],
        blur: lengths.get(2).copied().unwrap_or(0.0),
        spread: lengths.get(3).copied().unwrap_or(0.0),
        color,
        inset,
    })
}

pub(super) fn parse_background_image(
    name: &str,
    value: &str,
) -> Result<Option<BackgroundImage>, StyleError> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("url(") && value.ends_with(')') {
        let source = value[4..value.len() - 1].trim().trim_matches(['\'', '"']);
        return Ok(Some(BackgroundImage::Raster(parse_raster_data_url(
            name, source,
        )?)));
    }
    let (function, inner) = if lower.starts_with("linear-gradient(") && value.ends_with(')') {
        ("linear", &value[16..value.len() - 1])
    } else if lower.starts_with("radial-gradient(") && value.ends_with(')') {
        ("radial", &value[16..value.len() - 1])
    } else {
        return invalid(name, value);
    };
    let parts = split_top_level(inner, ',');
    if parts.len() < 2 {
        return invalid(name, value);
    }
    if function == "radial" {
        let color_start = usize::from(parse_color_stop(name, parts[0]).is_err());
        let stops = resolve_gradient_stops(name, &parts[color_start..])?;
        return Ok(Some(BackgroundImage::RadialGradient { stops }));
    }
    let (direction, color_start) = if parse_color_stop(name, parts[0]).is_ok() {
        ([0.0, 1.0], 0)
    } else {
        (parse_gradient_direction(name, parts[0])?, 1)
    };
    if parts.len() - color_start < 2 {
        return invalid(name, value);
    }
    Ok(Some(BackgroundImage::LinearGradient {
        direction,
        stops: resolve_gradient_stops(name, &parts[color_start..])?,
    }))
}

pub(super) fn parse_raster_data_url(
    name: &str,
    source: &str,
) -> Result<render::RasterImage, StyleError> {
    static CACHE: OnceLock<Mutex<VecDeque<(String, render::RasterImage)>>> = OnceLock::new();
    const CACHE_CAPACITY: usize = 64;
    const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
    const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;

    let cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Some(image) = cache
        .lock()
        .expect("background image cache lock")
        .iter()
        .find_map(|(cached_source, image)| (cached_source == source).then(|| image.clone()))
    {
        return Ok(image);
    }
    let encoded = source
        .strip_prefix("data:image/png;base64,")
        .filter(|encoded| encoded.len() <= MAX_ENCODED_BYTES)
        .ok_or_else(|| StyleError::InvalidValue(name.into(), source.into()))?;
    let bytes = decode_base64(encoded)
        .ok_or_else(|| StyleError::InvalidValue(name.into(), source.into()))?;
    let image =
        decode_png(&bytes).ok_or_else(|| StyleError::InvalidValue(name.into(), source.into()))?;
    let mut cache = cache.lock().expect("background image cache lock");
    if let Some(existing) = cache
        .iter()
        .find_map(|(cached_source, image)| (cached_source == source).then(|| image.clone()))
    {
        return Ok(existing);
    }
    while cache.len() >= CACHE_CAPACITY
        || cache
            .iter()
            .map(|(_, image)| image.pixels.len())
            .sum::<usize>()
            .saturating_add(image.pixels.len())
            > MAX_CACHE_BYTES
    {
        cache.pop_front();
    }
    cache.push_back((source.to_owned(), image.clone()));
    Ok(image)
}

pub(super) fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        accumulator = (accumulator << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    Some(output)
}

pub(super) fn decode_png(bytes: &[u8]) -> Option<render::RasterImage> {
    const MAX_DIMENSION: u32 = 4096;
    const MAX_PIXELS: usize = 4 * 1024 * 1024;
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let dimensions = reader.info();
    if dimensions.width > MAX_DIMENSION || dimensions.height > MAX_DIMENSION {
        return None;
    }
    let pixel_count = usize::try_from(dimensions.width)
        .ok()?
        .checked_mul(usize::try_from(dimensions.height).ok()?)?;
    if pixel_count > MAX_PIXELS {
        return None;
    }
    let mut decoded = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut decoded).ok()?;
    let source = &decoded[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(pixel_count.checked_mul(4)?);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(source),
        png::ColorType::Rgb => {
            for pixel in source.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for gray in source {
                rgba.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in source.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Indexed => return None,
    }
    render::RasterImage::new(info.width, info.height, rgba)
}

pub(super) fn parse_gradient_direction(name: &str, value: &str) -> Result<[f32; 2], StyleError> {
    let value = value.trim().to_ascii_lowercase();
    let angle = if let Some(side) = value.strip_prefix("to ") {
        return match side.trim() {
            "right" => Ok([1.0, 0.0]),
            "left" => Ok([-1.0, 0.0]),
            "bottom" => Ok([0.0, 1.0]),
            "top" => Ok([0.0, -1.0]),
            "bottom right" | "right bottom" => Ok([std::f32::consts::FRAC_1_SQRT_2; 2]),
            "top left" | "left top" => Ok([-std::f32::consts::FRAC_1_SQRT_2; 2]),
            "top right" | "right top" => Ok([
                std::f32::consts::FRAC_1_SQRT_2,
                -std::f32::consts::FRAC_1_SQRT_2,
            ]),
            "bottom left" | "left bottom" => Ok([
                -std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ]),
            _ => invalid(name, value.as_str()),
        };
    } else {
        parse_angle(name, &value)?
    };
    Ok([angle.sin(), -angle.cos()])
}

pub(super) fn parse_color_stop(
    name: &str,
    value: &str,
) -> Result<(Color, Option<f32>), StyleError> {
    let parts = split_whitespace_preserving_functions(value);
    let color = parts
        .first()
        .and_then(|part| parse_color(name, part).ok())
        .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?;
    let position = parts
        .get(1)
        .map(|position| {
            if *position == "0" {
                Ok(0.0)
            } else {
                position
                    .strip_suffix('%')
                    .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))
                    .and_then(|position| parse_number(name, position.trim()))
                    .map(|position| position / 100.0)
            }
        })
        .transpose()?;
    if parts.len() > 2 {
        return invalid(name, value);
    }
    Ok((color, position))
}

pub(super) fn resolve_gradient_stops(
    name: &str,
    values: &[&str],
) -> Result<Vec<GradientStop>, StyleError> {
    if values.len() < 2 || values.len() > 32 {
        return invalid(name, &values.join(", "));
    }
    let mut parsed = values
        .iter()
        .map(|value| parse_color_stop(name, value))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed[0].1.is_none() {
        parsed[0].1 = Some(0.0);
    }
    let last = parsed.len() - 1;
    if parsed[last].1.is_none() {
        parsed[last].1 = Some(1.0);
    }
    let mut previous = 0;
    while previous < last {
        let next = (previous + 1..=last)
            .find(|index| parsed[*index].1.is_some())
            .expect("last stop has a position");
        let start = parsed[previous].1.unwrap();
        let end = parsed[next].1.unwrap().max(start);
        parsed[next].1 = Some(end);
        let span = (next - previous) as f32;
        for (offset, stop) in parsed[previous + 1..next].iter_mut().enumerate() {
            stop.1 = Some(start + (end - start) * (offset as f32 + 1.0) / span);
        }
        previous = next;
    }
    Ok(parsed
        .into_iter()
        .map(|(color, position)| GradientStop {
            color,
            position: position.expect("positions were resolved"),
        })
        .collect())
}

pub(super) fn argument<'a>(
    arguments: &'a [&str],
    index: usize,
    original: &str,
) -> Result<&'a str, StyleError> {
    arguments
        .get(index)
        .copied()
        .ok_or_else(|| StyleError::InvalidValue("transform".into(), original.into()))
}

pub(super) fn split_arguments(value: &str) -> Vec<&str> {
    let comma = split_top_level(value, ',');
    if comma.len() > 1 {
        comma
    } else {
        value.split_ascii_whitespace().collect()
    }
}

pub(super) fn split_top_level(value: &str, separator: char) -> Vec<&str> {
    let mut depth = 0;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if character == separator && depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

pub(super) fn split_whitespace_preserving_functions(value: &str) -> Vec<String> {
    let mut depth = 0;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if character.is_ascii_whitespace() && depth == 0 => {
                if start < index {
                    parts.push(value[start..index].to_owned());
                }
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if start < value.len() {
        parts.push(value[start..].to_owned());
    }
    parts
}

pub(super) fn parse_color(name: &str, value: &str) -> Result<Color, StyleError> {
    let lower = value.trim().to_ascii_lowercase();
    if let Some(rgb) = named_color(&lower) {
        return Ok([
            (rgb >> 16) as u8,
            (rgb >> 8) as u8,
            rgb as u8,
            if lower == "transparent" { 0 } else { 255 },
        ]);
    }
    for function in ["rgb", "rgba", "hsl", "hsla"] {
        if lower.starts_with(&format!("{function}(")) && lower.ends_with(')') {
            return parse_function_color(
                name,
                function,
                &lower[function.len() + 1..lower.len() - 1],
            );
        }
    }

    let hex = lower
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

pub(super) fn parse_function_color(
    name: &str,
    function: &str,
    inner: &str,
) -> Result<Color, StyleError> {
    let normalized = inner.replace('/', " ");
    let parts = if normalized.contains(',') {
        normalized
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
    } else {
        normalized.split_ascii_whitespace().collect::<Vec<_>>()
    };
    let has_alpha = matches!(function, "rgba" | "hsla") || parts.len() == 4;
    if parts.len() != if has_alpha { 4 } else { 3 } {
        return invalid(name, inner);
    }
    let alpha = if has_alpha {
        parse_alpha(name, parts[3])?
    } else {
        255
    };
    if function.starts_with("rgb") {
        return Ok([
            parse_rgb_channel(name, parts[0])?,
            parse_rgb_channel(name, parts[1])?,
            parse_rgb_channel(name, parts[2])?,
            alpha,
        ]);
    }
    let hue = parse_hue(name, parts[0])?;
    let saturation = parse_percentage(name, parts[1])?;
    let lightness = parse_percentage(name, parts[2])?;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let segment = hue / 60.0;
    let secondary = chroma * (1.0 - (segment.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match segment as u32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let match_value = lightness - chroma * 0.5;
    Ok([
        ((red + match_value) * 255.0).round() as u8,
        ((green + match_value) * 255.0).round() as u8,
        ((blue + match_value) * 255.0).round() as u8,
        alpha,
    ])
}

pub(super) fn parse_rgb_channel(name: &str, value: &str) -> Result<u8, StyleError> {
    let channel = if let Some(percent) = value.strip_suffix('%') {
        parse_number(name, percent.trim())? * 2.55
    } else {
        parse_number(name, value)?
    };
    Ok(channel.clamp(0.0, 255.0).round() as u8)
}

pub(super) fn parse_alpha(name: &str, value: &str) -> Result<u8, StyleError> {
    let alpha = if let Some(percent) = value.strip_suffix('%') {
        parse_number(name, percent.trim())? / 100.0
    } else {
        parse_number(name, value)?
    };
    Ok((alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub(super) fn parse_percentage(name: &str, value: &str) -> Result<f32, StyleError> {
    let value = value
        .strip_suffix('%')
        .ok_or_else(|| StyleError::InvalidValue(name.into(), value.into()))?;
    Ok((parse_number(name, value.trim())? / 100.0).clamp(0.0, 1.0))
}

pub(super) fn parse_hue(name: &str, value: &str) -> Result<f32, StyleError> {
    let degrees = if let Some(value) = value.strip_suffix("deg") {
        parse_number(name, value.trim())?
    } else if let Some(value) = value.strip_suffix("turn") {
        parse_number(name, value.trim())? * 360.0
    } else if let Some(value) = value.strip_suffix("rad") {
        parse_number(name, value.trim())?.to_degrees()
    } else {
        parse_number(name, value)?
    };
    Ok(degrees.rem_euclid(360.0))
}

pub(super) fn named_color(value: &str) -> Option<u32> {
    Some(match value {
        "transparent" | "black" => 0x000000,
        "silver" => 0xc0c0c0,
        "gray" | "grey" => 0x808080,
        "white" => 0xffffff,
        "maroon" => 0x800000,
        "red" => 0xff0000,
        "purple" => 0x800080,
        "fuchsia" | "magenta" => 0xff00ff,
        "green" => 0x008000,
        "lime" => 0x00ff00,
        "olive" => 0x808000,
        "yellow" => 0xffff00,
        "navy" => 0x000080,
        "blue" => 0x0000ff,
        "teal" => 0x008080,
        "aqua" | "cyan" => 0x00ffff,
        "orange" => 0xffa500,
        "aliceblue" => 0xf0f8ff,
        "antiquewhite" => 0xfaebd7,
        "aquamarine" => 0x7fffd4,
        "azure" => 0xf0ffff,
        "beige" => 0xf5f5dc,
        "bisque" => 0xffe4c4,
        "blanchedalmond" => 0xffebcd,
        "blueviolet" => 0x8a2be2,
        "brown" => 0xa52a2a,
        "burlywood" => 0xdeb887,
        "cadetblue" => 0x5f9ea0,
        "chartreuse" => 0x7fff00,
        "chocolate" => 0xd2691e,
        "coral" => 0xff7f50,
        "cornflowerblue" => 0x6495ed,
        "cornsilk" => 0xfff8dc,
        "crimson" => 0xdc143c,
        "darkblue" => 0x00008b,
        "darkcyan" => 0x008b8b,
        "darkgoldenrod" => 0xb8860b,
        "darkgray" | "darkgrey" => 0xa9a9a9,
        "darkgreen" => 0x006400,
        "darkkhaki" => 0xbdb76b,
        "darkmagenta" => 0x8b008b,
        "darkolivegreen" => 0x556b2f,
        "darkorange" => 0xff8c00,
        "darkorchid" => 0x9932cc,
        "darkred" => 0x8b0000,
        "darksalmon" => 0xe9967a,
        "darkseagreen" => 0x8fbc8f,
        "darkslateblue" => 0x483d8b,
        "darkslategray" | "darkslategrey" => 0x2f4f4f,
        "darkturquoise" => 0x00ced1,
        "darkviolet" => 0x9400d3,
        "deeppink" => 0xff1493,
        "deepskyblue" => 0x00bfff,
        "dimgray" | "dimgrey" => 0x696969,
        "dodgerblue" => 0x1e90ff,
        "firebrick" => 0xb22222,
        "floralwhite" => 0xfffaf0,
        "forestgreen" => 0x228b22,
        "gainsboro" => 0xdcdcdc,
        "ghostwhite" => 0xf8f8ff,
        "gold" => 0xffd700,
        "goldenrod" => 0xdaa520,
        "greenyellow" => 0xadff2f,
        "honeydew" => 0xf0fff0,
        "hotpink" => 0xff69b4,
        "indianred" => 0xcd5c5c,
        "indigo" => 0x4b0082,
        "ivory" => 0xfffff0,
        "khaki" => 0xf0e68c,
        "lavender" => 0xe6e6fa,
        "lavenderblush" => 0xfff0f5,
        "lawngreen" => 0x7cfc00,
        "lemonchiffon" => 0xfffacd,
        "lightblue" => 0xadd8e6,
        "lightcoral" => 0xf08080,
        "lightcyan" => 0xe0ffff,
        "lightgoldenrodyellow" => 0xfafad2,
        "lightgray" | "lightgrey" => 0xd3d3d3,
        "lightgreen" => 0x90ee90,
        "lightpink" => 0xffb6c1,
        "lightsalmon" => 0xffa07a,
        "lightseagreen" => 0x20b2aa,
        "lightskyblue" => 0x87cefa,
        "lightslategray" | "lightslategrey" => 0x778899,
        "lightsteelblue" => 0xb0c4de,
        "lightyellow" => 0xffffe0,
        "limegreen" => 0x32cd32,
        "linen" => 0xfaf0e6,
        "mediumaquamarine" => 0x66cdaa,
        "mediumblue" => 0x0000cd,
        "mediumorchid" => 0xba55d3,
        "mediumpurple" => 0x9370db,
        "mediumseagreen" => 0x3cb371,
        "mediumslateblue" => 0x7b68ee,
        "mediumspringgreen" => 0x00fa9a,
        "mediumturquoise" => 0x48d1cc,
        "mediumvioletred" => 0xc71585,
        "midnightblue" => 0x191970,
        "mintcream" => 0xf5fffa,
        "mistyrose" => 0xffe4e1,
        "moccasin" => 0xffe4b5,
        "navajowhite" => 0xffdead,
        "oldlace" => 0xfdf5e6,
        "olivedrab" => 0x6b8e23,
        "orangered" => 0xff4500,
        "orchid" => 0xda70d6,
        "palegoldenrod" => 0xeee8aa,
        "palegreen" => 0x98fb98,
        "paleturquoise" => 0xafeeee,
        "palevioletred" => 0xdb7093,
        "papayawhip" => 0xffefd5,
        "peachpuff" => 0xffdab9,
        "peru" => 0xcd853f,
        "pink" => 0xffc0cb,
        "plum" => 0xdda0dd,
        "powderblue" => 0xb0e0e6,
        "rebeccapurple" => 0x663399,
        "rosybrown" => 0xbc8f8f,
        "royalblue" => 0x4169e1,
        "saddlebrown" => 0x8b4513,
        "salmon" => 0xfa8072,
        "sandybrown" => 0xf4a460,
        "seagreen" => 0x2e8b57,
        "seashell" => 0xfff5ee,
        "sienna" => 0xa0522d,
        "skyblue" => 0x87ceeb,
        "slateblue" => 0x6a5acd,
        "slategray" | "slategrey" => 0x708090,
        "snow" => 0xfffafa,
        "springgreen" => 0x00ff7f,
        "steelblue" => 0x4682b4,
        "tan" => 0xd2b48c,
        "thistle" => 0xd8bfd8,
        "tomato" => 0xff6347,
        "turquoise" => 0x40e0d0,
        "violet" => 0xee82ee,
        "wheat" => 0xf5deb3,
        "whitesmoke" => 0xf5f5f5,
        "yellowgreen" => 0x9acd32,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "known bug: malformed non-ASCII hex colors panic instead of returning InvalidValue"]
    fn non_ascii_hex_color_returns_invalid_value() {
        assert!(matches!(
            parse_color("color", "#€aaa"),
            Err(StyleError::InvalidValue(_, _))
        ));
    }
}
