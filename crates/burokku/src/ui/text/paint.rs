use glifo::{FontEmbolden, Glyph};
use parley::{fontique::Synthesis, FontData, PositionedLayoutItem};
use taffy::geometry::Point;
use vello_common::{
    kurbo::{Affine, Diagonal2},
    paint::Color,
};
use vello_hybrid::{Resources, Scene};

use super::{ShapedParagraph, TextBrush, TextError};

/// One owned glyph submission prepared from a Parley positioned run.
///
/// `FontData` cloning shares the underlying font blob; it does not copy bytes.
#[derive(Clone, Debug)]
pub(crate) struct GlyphBatch {
    font: FontData,
    font_size: f32,
    brush: TextBrush,
    normalized_coords: Vec<i16>,
    synthesis: Synthesis,
    glyphs: Vec<Glyph>,
}

impl GlyphBatch {
    pub(crate) fn font(&self) -> &FontData {
        &self.font
    }

    pub(crate) fn font_size(&self) -> f32 {
        self.font_size
    }

    pub(crate) fn brush(&self) -> TextBrush {
        self.brush
    }

    pub(crate) fn normalized_coords(&self) -> &[i16] {
        &self.normalized_coords
    }

    pub(crate) fn synthesis(&self) -> Synthesis {
        self.synthesis
    }

    pub(crate) fn glyphs(&self) -> &[Glyph] {
        &self.glyphs
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GlyphPaintStats {
    runs: usize,
    glyphs: usize,
}

impl GlyphPaintStats {
    pub(crate) fn runs(self) -> usize {
        self.runs
    }

    pub(crate) fn glyphs(self) -> usize {
        self.glyphs
    }
}

/// Prepare exact glyph runs from the shaped layout selected after Taffy.
///
/// Parley's `positioned_glyphs` already contains each run's horizontal offset
/// and baseline. This adapter adds only the paragraph content-box origin.
pub(crate) fn prepare_glyph_batches(
    content_origin: Point<f32>,
    paragraph: &ShapedParagraph,
) -> Result<Vec<GlyphBatch>, TextError> {
    validate_coordinate(paragraph, "content origin x", content_origin.x)?;
    validate_coordinate(paragraph, "content origin y", content_origin.y)?;

    let mut batches = Vec::new();
    for line in paragraph.layout().lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let font_size = run.run().font_size();
            validate_non_negative(paragraph, "font size", font_size)?;

            let mut glyphs = Vec::new();
            for glyph in run.positioned_glyphs() {
                let x = content_origin.x + glyph.x;
                let y = content_origin.y + glyph.y;
                validate_coordinate(paragraph, "glyph x", x)?;
                validate_coordinate(paragraph, "glyph y", y)?;
                glyphs.push(Glyph { id: glyph.id, x, y });
            }
            if glyphs.is_empty() {
                continue;
            }

            batches.push(GlyphBatch {
                font: run.run().font().clone(),
                font_size,
                brush: run.style().brush,
                normalized_coords: run.run().normalized_coords().to_vec(),
                synthesis: run.run().synthesis(),
                glyphs,
            });
        }
    }
    Ok(batches)
}

/// Submit an already-selected shaped paragraph to renderer-owned Vello state.
pub(crate) fn paint_paragraph(
    scene: &mut Scene,
    resources: &mut Resources,
    content_origin: Point<f32>,
    paragraph: &ShapedParagraph,
) -> Result<GlyphPaintStats, TextError> {
    let batches = prepare_glyph_batches(content_origin, paragraph)?;
    let mut stats = GlyphPaintStats::default();

    for batch in batches {
        let [red, green, blue, alpha] = batch.brush;
        scene.set_paint(Color::from_rgba8(red, green, blue, alpha));
        let mut builder = scene
            .glyph_run(resources, &batch.font)
            .font_size(batch.font_size)
            .normalized_coords(&batch.normalized_coords)
            .hint(false);

        if batch.synthesis.embolden() {
            // Fontique reports only whether faux bold is needed. A small
            // em-relative expansion matches the selected logical font size and
            // remains independent of display scale.
            let amount = f64::from(batch.font_size) / 32.0;
            builder = builder.font_embolden(FontEmbolden::new(Diagonal2::new(amount, amount)));
        }
        if let Some(degrees) = batch.synthesis.skew() {
            let skew = f64::from(degrees).to_radians().tan();
            builder = builder.glyph_transform(Affine::skew(skew, 0.0));
        }

        stats.runs += 1;
        stats.glyphs += batch.glyphs.len();
        builder.fill_glyphs(batch.glyphs.into_iter());
    }
    Ok(stats)
}

fn validate_coordinate(
    paragraph: &ShapedParagraph,
    field: &'static str,
    value: f32,
) -> Result<(), TextError> {
    if !value.is_finite() {
        return Err(TextError::InvalidMetric {
            paragraph: paragraph.source(),
            field,
            value,
        });
    }
    Ok(())
}

fn validate_non_negative(
    paragraph: &ShapedParagraph,
    field: &'static str,
    value: f32,
) -> Result<(), TextError> {
    validate_coordinate(paragraph, field, value)?;
    if value < 0.0 {
        return Err(TextError::InvalidMetric {
            paragraph: paragraph.source(),
            field,
            value,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::ui::{
        elements::{
            styles::{color::RgbaColor, text::ComputedTextStyle},
            Dom, Element, ElementTag,
        },
        text::{ParagraphInput, StyledTextRun, TextConstraint, TextEngine},
    };

    use super::*;

    const TEST_FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSans-Regular.ttf");

    fn paragraph_input() -> ParagraphInput {
        let mut dom = Dom::new();
        let source = dom.create_element(Element::from_tag(ElementTag::Text));
        let base = ComputedTextStyle {
            font_family: "Noto Sans".into(),
            ..ComputedTextStyle::default()
        };
        let mut red = base.clone();
        red.color = RgbaColor::rgb(255, 0, 0);
        let mut blue = base.clone();
        blue.color = RgbaColor::rgb(0, 0, 255);
        ParagraphInput::new(
            source,
            base,
            "red blue".into(),
            vec![
                StyledTextRun::new(0..4, red),
                StyledTextRun::new(4..8, blue),
            ],
        )
    }

    fn shaped() -> Rc<ShapedParagraph> {
        let mut engine = TextEngine::without_system_fonts();
        engine.register_font_data(TEST_FONT.to_vec()).unwrap();
        engine
            .shape(&paragraph_input(), TextConstraint::definite(200.0).unwrap())
            .unwrap()
    }

    #[test]
    fn prepares_colored_glyph_runs_from_the_selected_layout() {
        let shaped = shaped();

        let batches = prepare_glyph_batches(Point { x: 10.0, y: 20.0 }, &shaped).unwrap();

        assert!(!batches.is_empty());
        assert!(batches
            .iter()
            .any(|batch| batch.brush() == [255, 0, 0, 255]));
        assert!(batches
            .iter()
            .any(|batch| batch.brush() == [0, 0, 255, 255]));
        assert!(batches.iter().all(|batch| batch.font_size() == 16.0));
        assert!(batches.iter().all(|batch| !batch.glyphs().is_empty()));
        assert!(batches
            .iter()
            .all(|batch| !batch.font().data.data().is_empty()));
    }

    #[test]
    fn adds_the_content_origin_once_to_parley_positioned_glyphs() {
        let shaped = shaped();
        let zero = prepare_glyph_batches(Point::ZERO, &shaped).unwrap();
        let translated = prepare_glyph_batches(Point { x: 7.5, y: 11.25 }, &shaped).unwrap();

        let zero_glyphs = zero
            .iter()
            .flat_map(|batch| batch.glyphs())
            .collect::<Vec<_>>();
        let translated_glyphs = translated
            .iter()
            .flat_map(|batch| batch.glyphs())
            .collect::<Vec<_>>();
        assert_eq!(zero_glyphs.len(), translated_glyphs.len());
        for (zero, translated) in zero_glyphs.into_iter().zip(translated_glyphs) {
            assert_eq!(zero.id, translated.id);
            assert!((translated.x - zero.x - 7.5).abs() < 0.001);
            assert!((translated.y - zero.y - 11.25).abs() < 0.001);
        }
    }

    #[test]
    fn preserves_normalized_coordinates_and_synthesis_metadata() {
        let shaped = shaped();
        let batches = prepare_glyph_batches(Point::ZERO, &shaped).unwrap();

        for batch in batches {
            assert!(batch.normalized_coords().len() < 64);
            let _synthesis = batch.synthesis();
        }
    }

    #[test]
    fn rejects_non_finite_content_origins() {
        let shaped = shaped();

        let error = prepare_glyph_batches(
            Point {
                x: f32::NAN,
                y: 0.0,
            },
            &shaped,
        )
        .unwrap_err();

        assert!(matches!(error, TextError::InvalidMetric { .. }));
    }
}
