use glifo::{FontEmbolden, Glyph};
use parley::{fontique::Synthesis, FontData, PositionedLayoutItem};
use skrifa::{raw::TableProvider as _, FontRef};
use taffy::geometry::Point;
use vello_common::{
    kurbo::{Affine, Diagonal2, Rect, Stroke},
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreparedReplacementBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    stroke_width: f32,
    brush: TextBrush,
}

impl PreparedReplacementBox {
    pub(crate) fn x(self) -> f32 {
        self.x
    }

    pub(crate) fn y(self) -> f32 {
        self.y
    }

    pub(crate) fn width(self) -> f32 {
        self.width
    }

    pub(crate) fn height(self) -> f32 {
        self.height
    }

    pub(crate) fn stroke_width(self) -> f32 {
        self.stroke_width
    }

    pub(crate) fn brush(self) -> TextBrush {
        self.brush
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

/// Translate synthetic missing-glyph boxes into scene coordinates.
pub(crate) fn prepare_replacement_boxes(
    content_origin: Point<f32>,
    paragraph: &ShapedParagraph,
) -> Result<Vec<PreparedReplacementBox>, TextError> {
    validate_coordinate(paragraph, "content origin x", content_origin.x)?;
    validate_coordinate(paragraph, "content origin y", content_origin.y)?;

    let mut prepared = Vec::new();
    for line in paragraph.layout().lines() {
        for item in line.items() {
            let PositionedLayoutItem::InlineBox(positioned) = item else {
                continue;
            };
            let index = usize::try_from(positioned.id)
                .expect("replacement box IDs fit usize on supported platforms");
            let replacement = paragraph
                .replacement_boxes()
                .get(index)
                .expect("replacement box IDs match retained paint metadata");
            if !replacement.visible() {
                continue;
            }
            let x = content_origin.x + positioned.x;
            let y = content_origin.y + positioned.y;
            validate_coordinate(paragraph, "replacement box x", x)?;
            validate_coordinate(paragraph, "replacement box y", y)?;
            validate_non_negative(paragraph, "replacement box width", positioned.width)?;
            validate_non_negative(paragraph, "replacement box height", positioned.height)?;
            prepared.push(PreparedReplacementBox {
                x,
                y,
                width: positioned.width,
                height: positioned.height,
                stroke_width: replacement.stroke_width(),
                brush: replacement.brush(),
            });
        }
    }
    Ok(prepared)
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
            builder = builder.font_embolden(faux_bold_embolden(&batch.font)?);
        }
        if let Some(degrees) = batch.synthesis.skew() {
            let skew = f64::from(degrees).to_radians().tan();
            builder = builder.glyph_transform(Affine::skew(skew, 0.0));
        }

        stats.runs += 1;
        stats.glyphs += batch.glyphs.len();
        builder.fill_glyphs(batch.glyphs.into_iter());
    }

    for replacement in prepare_replacement_boxes(content_origin, paragraph)? {
        let [red, green, blue, alpha] = replacement.brush();
        scene.set_paint(Color::from_rgba8(red, green, blue, alpha));
        scene.set_stroke(Stroke::new(f64::from(replacement.stroke_width())));
        scene.stroke_rect(&Rect::new(
            f64::from(replacement.x()),
            f64::from(replacement.y()),
            f64::from(replacement.x() + replacement.width()),
            f64::from(replacement.y() + replacement.height()),
        ));
    }
    Ok(stats)
}

/// Glifo caches unhinted outlines at the font's UPEM size and scales them to
/// the requested pixel size while drawing. Expressing the expansion in font
/// units therefore produces a `font_size / 32` expansion in logical pixels.
fn faux_bold_embolden(font: &FontData) -> Result<FontEmbolden, TextError> {
    let font_ref = FontRef::from_index(font.data.data(), font.index)
        .map_err(|_| TextError::InvalidFontData)?;
    let units_per_em = font_ref
        .head()
        .map_err(|_| TextError::InvalidFontData)?
        .units_per_em();
    let amount = f64::from(units_per_em) / 32.0;
    Ok(FontEmbolden::new(Diagonal2::new(amount, amount)))
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
            styles::{
                color::RgbaColor,
                text::{ComputedTextStyle, FontWeight},
            },
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
        shape(paragraph_input())
    }

    fn shape(input: ParagraphInput) -> Rc<ShapedParagraph> {
        let mut engine = TextEngine::without_system_fonts();
        engine.register_font_data(TEST_FONT.to_vec()).unwrap();
        engine
            .shape(&input, TextConstraint::definite(200.0).unwrap())
            .unwrap()
    }

    fn bold_paragraph_input() -> ParagraphInput {
        let mut dom = Dom::new();
        let source = dom.create_element(Element::from_tag(ElementTag::Text));
        let style = ComputedTextStyle {
            font_family: "Noto Sans".into(),
            font_weight: FontWeight::BOLD,
            ..ComputedTextStyle::default()
        };
        ParagraphInput::new(
            source,
            style.clone(),
            "Bold".into(),
            vec![StyledTextRun::new(0..4, style)],
        )
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
    fn regular_font_synthesizes_requested_bold_weight() {
        let shaped = shape(bold_paragraph_input());
        let batches = prepare_glyph_batches(Point::ZERO, &shaped).unwrap();

        assert!(!batches.is_empty());
        assert!(batches.iter().all(|batch| batch.synthesis().embolden()));
    }

    #[test]
    fn faux_bold_expands_16px_ink_bounds_by_one_pixel() {
        use vello_common::kurbo::{expand_path, Join, Shape};

        let shaped = shape(bold_paragraph_input());
        let batch = prepare_glyph_batches(Point::ZERO, &shaped)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert!(batch.synthesis().embolden());

        let embolden = faux_bold_embolden(batch.font()).unwrap();
        let regular = Rect::new(0.0, 0.0, 500.0, 700.0).to_path(0.1);
        let regular_bounds = regular.bounding_box();
        let bold_bounds = expand_path(
            regular,
            embolden.amount,
            Join::Miter,
            embolden.miter_limit,
            embolden.tolerance,
        )
        .bounding_box();
        let font_ref = FontRef::from_index(batch.font().data.data(), batch.font().index).unwrap();
        let units_per_em = f64::from(font_ref.head().unwrap().units_per_em());
        let pixels_per_font_unit = f64::from(batch.font_size()) / units_per_em;
        let pixel_width_increase =
            (bold_bounds.width() - regular_bounds.width()) * pixels_per_font_unit;

        assert!((pixel_width_increase - 1.0).abs() < 0.01);
    }

    #[test]
    fn translates_missing_font_replacement_boxes() {
        let mut engine = TextEngine::without_system_fonts();
        let paragraph = engine
            .shape(&paragraph_input(), TextConstraint::definite(200.0).unwrap())
            .unwrap();
        let zero = prepare_replacement_boxes(Point::ZERO, &paragraph).unwrap();
        let translated = prepare_replacement_boxes(Point { x: 7.5, y: 11.25 }, &paragraph).unwrap();

        assert!(!zero.is_empty());
        assert!(zero
            .iter()
            .any(|replacement| replacement.brush() == [255, 0, 0, 255]));
        assert!(zero
            .iter()
            .any(|replacement| replacement.brush() == [0, 0, 255, 255]));
        assert_eq!(zero.len(), translated.len());
        for (zero, translated) in zero.into_iter().zip(translated) {
            assert!((translated.x() - zero.x() - 7.5).abs() < 0.001);
            assert!((translated.y() - zero.y() - 11.25).abs() < 0.001);
            assert_eq!(translated.width(), zero.width());
            assert_eq!(translated.height(), zero.height());
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
