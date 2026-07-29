use render::{
    FontFamily, TextOverflowWrap, TextSpan, TextStyle, TextWhiteSpace, TextWordBreak, TextWrap,
    Transform,
};

use crate::ui::elements::styles::{LengthPercentageValue, LineHeightValue, Style as ElementStyle};

use super::paint::rgba;

pub(in crate::ui::layouts) fn merge_text_style(
    parent: &TextStyle,
    style: &ElementStyle,
) -> TextStyle {
    let mut merged = parent.clone();
    if let Some(color) = style.color {
        merged.color = rgba(color);
        if merged.text_decoration_color_is_current {
            merged.text_decoration_color = merged.color;
        }
    }
    if let Some(font_size) = style.font_size {
        merged.font_size = match font_size {
            LengthPercentageValue::Px(value) => value,
            LengthPercentageValue::Percent(value) => parent.font_size * value / 100.0,
        };
        if merged.line_height_is_normal && style.line_height.is_none() {
            merged.line_height = merged.font_size * 1.2;
        }
    }
    if let Some(line_height) = style.line_height {
        merged.line_height_is_normal = line_height == LineHeightValue::Normal;
        merged.line_height = match line_height {
            LineHeightValue::Normal => merged.font_size * 1.2,
            LineHeightValue::Number(value) => merged.font_size * value,
            LineHeightValue::Px(value) => value,
            LineHeightValue::Percent(value) => merged.font_size * value / 100.0,
        };
    }
    if let Some(font_weight) = style.font_weight {
        merged.font_weight = font_weight;
    }
    if let Some(font_families) = &style.font_families {
        merged.font_families = font_families
            .iter()
            .map(|family| font_family(family))
            .collect();
    }
    if let Some(font_style) = style.font_style {
        merged.font_style = font_style.into();
    }
    if let Some(text_align) = style.text_align {
        merged.text_align = text_align.into();
    }
    if let Some(letter_spacing) = style.letter_spacing {
        merged.letter_spacing = letter_spacing.into();
    }
    if let Some(word_spacing) = style.word_spacing {
        merged.word_spacing = word_spacing.into();
    }
    if let Some(line) = style.text_decoration_line {
        merged.text_decoration_line = line.into();
    }
    if let Some(color) = style.text_decoration_color {
        merged.text_decoration_color = rgba(color);
        merged.text_decoration_color_is_current = false;
    }
    if let Some(white_space) = style.white_space {
        merged.white_space = white_space.into();
    }
    if let Some(overflow_wrap) = style.overflow_wrap {
        merged.overflow_wrap = overflow_wrap.into();
    }
    if let Some(word_break) = style.word_break {
        merged.word_break = word_break.into();
    }
    merged.opacity = style.opacity;
    merged.transform = Transform::IDENTITY;
    merged.shadows = style.text_shadow.iter().copied().map(Into::into).collect();
    merged.wrap = resolve_text_wrap(&merged);
    merged
}

fn resolve_text_wrap(style: &TextStyle) -> TextWrap {
    let wrapping_allowed = !matches!(
        style.white_space,
        TextWhiteSpace::NoWrap | TextWhiteSpace::Pre
    );
    if !wrapping_allowed {
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

fn font_family(family: &str) -> FontFamily {
    match family.to_ascii_lowercase().as_str() {
        "serif" => FontFamily::Serif,
        "sans-serif" => FontFamily::SansSerif,
        "monospace" => FontFamily::Monospace,
        "cursive" => FontFamily::Cursive,
        "fantasy" => FontFamily::Fantasy,
        _ => FontFamily::Named(family.to_owned()),
    }
}

pub(in crate::ui::layouts) fn normalize_white_space(text: &str, mode: TextWhiteSpace) -> String {
    match mode {
        TextWhiteSpace::Pre | TextWhiteSpace::PreWrap | TextWhiteSpace::BreakSpaces => {
            text.to_owned()
        }
        TextWhiteSpace::Normal | TextWhiteSpace::NoWrap => collapse_white_space(text, false),
        TextWhiteSpace::PreLine => collapse_white_space(text, true),
    }
}

pub(in crate::ui::layouts) fn normalize_text_spans(
    spans: &[TextSpan],
    mode: TextWhiteSpace,
) -> Vec<TextSpan> {
    if matches!(
        mode,
        TextWhiteSpace::Pre | TextWhiteSpace::PreWrap | TextWhiteSpace::BreakSpaces
    ) {
        return spans.to_vec();
    }

    let preserve_newlines = mode == TextWhiteSpace::PreLine;
    let mut result = Vec::new();
    let mut pending_space = None;
    let mut has_text = false;
    let mut ends_with_newline = false;

    for span in spans {
        for character in span.text.chars() {
            if character == '\n' && preserve_newlines {
                append_styled_character(&mut result, '\n', &span.style);
                pending_space = None;
                has_text = true;
                ends_with_newline = true;
            } else if character.is_whitespace() {
                if has_text && !ends_with_newline && pending_space.is_none() {
                    pending_space = Some(span.style.clone());
                }
            } else {
                if let Some(space_style) = pending_space.take() {
                    append_styled_character(&mut result, ' ', &space_style);
                }
                append_styled_character(&mut result, character, &span.style);
                has_text = true;
                ends_with_newline = false;
            }
        }
    }
    result
}

fn append_styled_character(spans: &mut Vec<TextSpan>, character: char, style: &TextStyle) {
    if let Some(last) = spans.last_mut().filter(|span| span.style == *style) {
        last.text.push(character);
    } else {
        spans.push(TextSpan::new(character.to_string(), style.clone()));
    }
}

fn collapse_white_space(text: &str, preserve_newlines: bool) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character == '\n' && preserve_newlines {
            while result.ends_with(' ') {
                result.pop();
            }
            result.push('\n');
            pending_space = false;
        } else if character.is_whitespace() {
            pending_space = !result.is_empty() && !result.ends_with('\n');
        } else {
            if pending_space {
                result.push(' ');
            }
            result.push(character);
            pending_space = false;
        }
    }
    result
}
