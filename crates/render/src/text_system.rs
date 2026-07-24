use glyphon::{
    cosmic_text::Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style as GlyphStyle,
    Weight, Wrap,
};

use crate::{FontFamily, FontStyle, TextAlign, TextStyle, TextWrap};

/// Width behavior used while calculating intrinsic text dimensions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextWidth {
    /// Measure the maximum-content width without wrapping.
    #[default]
    Unconstrained,
    /// Wrap within the given width.
    AtMost(f32),
    /// Calculate the minimum-content width using the text's wrap opportunities.
    MinContent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextConstraints {
    pub width: TextWidth,
}

impl TextConstraints {
    pub const UNCONSTRAINED: Self = Self {
        width: TextWidth::Unconstrained,
    };

    pub const MIN_CONTENT: Self = Self {
        width: TextWidth::MinContent,
    };

    pub const fn at_most(width: f32) -> Self {
        Self {
            width: TextWidth::AtMost(width),
        }
    }
}

/// Dimensions produced by the same text layout engine used for rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub first_baseline: f32,
    pub line_count: usize,
}

/// Shaped geometry for a contiguous font run, used to paint text decorations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextRunMetrics {
    pub left: f32,
    pub width: f32,
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
    pub overline_y: f32,
    pub underline_y: f32,
    pub line_through_y: f32,
    pub overline_thickness: f32,
    pub underline_thickness: f32,
    pub line_through_thickness: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayoutMetrics {
    pub text: TextMetrics,
    pub runs: Vec<TextRunMetrics>,
}

/// Font database and shaping state shared by layout measurement and rendering.
pub struct TextSystem {
    font_system: FontSystem,
}

impl TextSystem {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
        }
    }

    /// Shapes and measures text without requiring a GPU device or surface.
    pub fn measure(
        &mut self,
        text: &str,
        style: &TextStyle,
        constraints: TextConstraints,
    ) -> TextMetrics {
        self.layout_metrics(text, style, constraints).text
    }

    /// Shapes text and returns aggregate dimensions plus per-font-run geometry.
    pub fn layout_metrics(
        &mut self,
        text: &str,
        style: &TextStyle,
        constraints: TextConstraints,
    ) -> TextLayoutMetrics {
        let (width, wrap) = match constraints.width {
            TextWidth::Unconstrained => (None, Wrap::None),
            TextWidth::AtMost(width) => (Some(width.max(0.0)), wrap_mode(style.wrap)),
            TextWidth::MinContent => (Some(0.0), wrap_mode(style.wrap)),
        };
        let buffer = self.layout_buffer(text, style, width, None, Some(wrap));
        let mut result = TextLayoutMetrics::default();
        for run in buffer.layout_runs() {
            if result.text.line_count == 0 {
                result.text.first_baseline = run.line_y;
            }
            result.text.width = result.text.width.max(run.line_w);
            result.text.height = result.text.height.max(run.line_top + run.line_height);
            result.text.line_count += 1;

            let mut start = 0;
            while start < run.glyphs.len() {
                let first = &run.glyphs[start];
                let mut end = start + 1;
                while end < run.glyphs.len() {
                    let glyph = &run.glyphs[end];
                    if glyph.font_id != first.font_id
                        || glyph.font_weight != first.font_weight
                        || glyph.font_size.to_bits() != first.font_size.to_bits()
                        || glyph.level != first.level
                    {
                        break;
                    }
                    end += 1;
                }
                let glyphs = &run.glyphs[start..end];
                let left = glyphs
                    .iter()
                    .map(|glyph| glyph.x)
                    .fold(f32::INFINITY, f32::min);
                let right = glyphs
                    .iter()
                    .map(|glyph| glyph.x + glyph.w)
                    .fold(f32::NEG_INFINITY, f32::max);
                if right > left {
                    result.runs.extend(text_run_metrics(
                        &mut self.font_system,
                        first,
                        run.line_y,
                        left,
                        right - left,
                    ));
                }
                start = end;
            }
        }
        result
    }

    pub(crate) fn layout_buffer(
        &mut self,
        text: &str,
        style: &TextStyle,
        width: Option<f32>,
        height: Option<f32>,
        wrap: Option<Wrap>,
    ) -> Buffer {
        let font_size = style.font_size.max(1.0);
        let line_height = style.line_height.max(font_size);
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));
        buffer.set_size(&mut self.font_system, width, height);
        buffer.set_wrap(
            &mut self.font_system,
            wrap.unwrap_or_else(|| wrap_mode(style.wrap)),
        );
        let family = match self.resolve_font_family(&style.font_families) {
            FontFamily::SansSerif => Family::SansSerif,
            FontFamily::Serif => Family::Serif,
            FontFamily::Monospace => Family::Monospace,
            FontFamily::Cursive => Family::Cursive,
            FontFamily::Fantasy => Family::Fantasy,
            FontFamily::Named(name) => Family::Name(name),
        };
        let attrs = Attrs::new()
            .family(family)
            .weight(Weight(style.font_weight))
            .style(match style.font_style {
                FontStyle::Normal => GlyphStyle::Normal,
                FontStyle::Italic => GlyphStyle::Italic,
                FontStyle::Oblique => GlyphStyle::Oblique,
            })
            .letter_spacing(style.letter_spacing / font_size);
        let alignment = match style.text_align {
            TextAlign::Start => None,
            TextAlign::End => Some(Align::End),
            TextAlign::Left => Some(Align::Left),
            TextAlign::Right => Some(Align::Right),
            TextAlign::Center => Some(Align::Center),
            TextAlign::Justify => Some(Align::Justified),
        };
        if style.word_spacing == 0.0 {
            buffer.set_text(
                &mut self.font_system,
                text,
                &attrs,
                Shaping::Advanced,
                alignment,
            );
        } else {
            let word_attrs = attrs
                .clone()
                .letter_spacing((style.letter_spacing + style.word_spacing) / font_size);
            let spans = text_spans(text, attrs.clone(), word_attrs);
            buffer.set_rich_text(
                &mut self.font_system,
                spans,
                &attrs,
                Shaping::Advanced,
                alignment,
            );
        }
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }

    fn resolve_font_family<'a>(&self, families: &'a [FontFamily]) -> &'a FontFamily {
        families
            .iter()
            .find(|family| match family {
                FontFamily::Named(requested) => self.font_system.db().faces().any(|face| {
                    face.families
                        .iter()
                        .any(|(name, _)| name.eq_ignore_ascii_case(requested))
                }),
                _ => true,
            })
            .unwrap_or(&DEFAULT_FONT_FAMILY)
    }

    pub(crate) fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }
}

fn text_run_metrics(
    font_system: &mut FontSystem,
    glyph: &glyphon::LayoutGlyph,
    baseline: f32,
    left: f32,
    width: f32,
) -> Option<TextRunMetrics> {
    let font = font_system.get_font(glyph.font_id, glyph.font_weight)?;
    let metrics = font.metrics();
    let scale = glyph.font_size / f32::from(metrics.units_per_em.max(1));
    let ascent = (metrics.ascent * scale).max(0.0);
    let descent = (-metrics.descent * scale).max(0.0);
    let em_thickness = (glyph.font_size / 16.0).max(f32::EPSILON);
    let underline = metrics.underline.map(|decoration| {
        (
            baseline - decoration.offset * scale,
            nonzero_thickness(decoration.thickness * scale, em_thickness),
        )
    });
    let strikeout = metrics.strikeout.map(|decoration| {
        (
            baseline - decoration.offset * scale,
            nonzero_thickness(decoration.thickness * scale, em_thickness),
        )
    });
    let (underline_y, underline_thickness) =
        underline.unwrap_or((baseline + descent * 0.5, em_thickness));
    let x_height = metrics.x_height.map(|height| height * scale);
    let (line_through_y, line_through_thickness) = strikeout.unwrap_or((
        baseline - x_height.unwrap_or(ascent * 0.5) * 0.5,
        em_thickness,
    ));

    Some(TextRunMetrics {
        left,
        width,
        baseline,
        ascent,
        descent,
        overline_y: baseline - ascent,
        underline_y,
        line_through_y,
        overline_thickness: underline_thickness,
        underline_thickness,
        line_through_thickness,
    })
}

fn nonzero_thickness(value: f32, fallback: f32) -> f32 {
    let value = value.abs();
    if value > f32::EPSILON {
        value
    } else {
        fallback
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) const fn wrap_mode(wrap: TextWrap) -> Wrap {
    match wrap {
        TextWrap::None => Wrap::None,
        TextWrap::Glyph => Wrap::Glyph,
        TextWrap::Word => Wrap::Word,
        TextWrap::WordOrGlyph => Wrap::WordOrGlyph,
    }
}

static DEFAULT_FONT_FAMILY: FontFamily = FontFamily::SansSerif;

fn text_spans<'a>(
    text: &'a str,
    normal: Attrs<'a>,
    spaced_word: Attrs<'a>,
) -> Vec<(&'a str, Attrs<'a>)> {
    let mut spans = Vec::new();
    let mut start = 0;
    let mut whitespace = None;
    for (index, character) in text.char_indices() {
        let is_word_separator = matches!(character, ' ' | '\t');
        match whitespace {
            None => whitespace = Some(is_word_separator),
            Some(current) if current != is_word_separator => {
                spans.push((
                    &text[start..index],
                    if current {
                        spaced_word.clone()
                    } else {
                        normal.clone()
                    },
                ));
                start = index;
                whitespace = Some(is_word_separator);
            }
            _ => {}
        }
    }
    if start < text.len() {
        spans.push((
            &text[start..],
            if whitespace.unwrap_or(false) {
                spaced_word
            } else {
                normal
            },
        ));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_wrapped_text_for_layout_engines() {
        let mut system = TextSystem::new();
        let style = TextStyle {
            font_size: 20.0,
            line_height: 24.0,
            ..TextStyle::default()
        };
        let unconstrained = system.measure(
            "Burokku text layout",
            &style,
            TextConstraints::UNCONSTRAINED,
        );
        if unconstrained.width == 0.0 {
            return;
        }
        let wrapped = system.measure(
            "Burokku text layout",
            &style,
            TextConstraints::at_most(unconstrained.width * 0.5),
        );
        assert!(wrapped.width < unconstrained.width);
        assert!(wrapped.height > unconstrained.height);
        assert!(wrapped.line_count > unconstrained.line_count);
    }

    #[test]
    fn applies_letter_and_word_spacing_to_measurement() {
        let mut system = TextSystem::new();
        let plain = system.measure("a a", &TextStyle::default(), TextConstraints::UNCONSTRAINED);
        if plain.width == 0.0 {
            return;
        }
        let spaced = system.measure(
            "a a",
            &TextStyle {
                letter_spacing: 2.0,
                word_spacing: 5.0,
                ..TextStyle::default()
            },
            TextConstraints::UNCONSTRAINED,
        );

        assert!(spaced.width > plain.width + 5.0);
    }

    #[test]
    fn chooses_the_first_available_font_family_fallback() {
        let system = TextSystem::new();
        let families = [
            FontFamily::Named("A font that cannot exist".to_owned()),
            FontFamily::Serif,
            FontFamily::SansSerif,
        ];

        assert_eq!(system.resolve_font_family(&families), &FontFamily::Serif);
    }

    #[test]
    fn applies_text_alignment_to_glyph_positions() {
        let mut system = TextSystem::new();
        let left = system.layout_buffer(
            "aligned",
            &TextStyle {
                text_align: TextAlign::Left,
                ..TextStyle::default()
            },
            Some(200.0),
            None,
            None,
        );
        let Some(left_x) = left
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map(|glyph| glyph.x)
        else {
            return;
        };
        let right = system.layout_buffer(
            "aligned",
            &TextStyle {
                text_align: TextAlign::Right,
                ..TextStyle::default()
            },
            Some(200.0),
            None,
            None,
        );
        let right_x = right
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .expect("the same text should shape")
            .x;

        assert!(right_x > left_x + 50.0);
    }

    #[test]
    fn decoration_runs_follow_aligned_and_wrapped_glyph_extents() {
        let mut system = TextSystem::new();
        let centered = system.layout_metrics(
            "decorated",
            &TextStyle {
                text_align: TextAlign::Center,
                ..TextStyle::default()
            },
            TextConstraints::at_most(200.0),
        );
        let right = system.layout_metrics(
            "decorated",
            &TextStyle {
                text_align: TextAlign::Right,
                ..TextStyle::default()
            },
            TextConstraints::at_most(200.0),
        );
        if centered.runs.is_empty() || right.runs.is_empty() {
            return;
        }

        assert!(centered.runs[0].left > 0.0);
        assert!(right.runs[0].left > centered.runs[0].left);
        assert!(centered.runs[0].width < 200.0);
        assert!(centered.runs[0].baseline > centered.runs[0].ascent);
        assert!(centered.runs[0].underline_y >= centered.runs[0].baseline);

        let wrapped = system.layout_metrics(
            "decorations follow each wrapped shaped line",
            &TextStyle {
                text_align: TextAlign::Center,
                ..TextStyle::default()
            },
            TextConstraints::at_most(90.0),
        );
        assert!(wrapped.text.line_count > 1);
        assert!(wrapped.runs.len() >= wrapped.text.line_count);
        assert!(wrapped
            .runs
            .windows(2)
            .any(|runs| runs[0].baseline != runs[1].baseline));
        assert!(wrapped
            .runs
            .iter()
            .all(|run| run.left >= 0.0 && run.left + run.width <= 90.01));
    }
}
