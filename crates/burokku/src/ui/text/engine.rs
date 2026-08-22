use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    rc::Rc,
};

#[cfg(test)]
use parley::fontique::{Collection, CollectionOptions, SourceCache};
use parley::{
    fontique::Blob, Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout,
    LayoutContext, LineHeight as ParleyLineHeight, StyleProperty, TextWrapMode,
};

use crate::ui::elements::{
    styles::text::{ComputedTextStyle, LineHeight, TextWrap},
    NodeId,
};

use super::{ParagraphInput, TextError, TextFingerprint};

pub(crate) type TextBrush = [u8; 4];

const SHAPING_CONFIGURATION_VERSION: u64 = 1;
const MAX_WIDTH_VARIANTS: usize = 4;

/// Width semantics requested by Taffy for an intrinsic text measurement.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextConstraint {
    MinContent,
    MaxContent,
    Definite(CanonicalWidth),
}

impl TextConstraint {
    pub(crate) fn definite(width: f32) -> Result<Self, TextError> {
        CanonicalWidth::new(width).map(Self::Definite)
    }

    pub(crate) fn definite_value(self) -> Option<f32> {
        match self {
            Self::Definite(width) => Some(width.get()),
            Self::MinContent | Self::MaxContent => None,
        }
    }
}

/// Canonical finite non-negative logical width represented by exact bits.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CanonicalWidth(u32);

impl CanonicalWidth {
    fn new(width: f32) -> Result<Self, TextError> {
        if !width.is_finite() || width < 0.0 {
            return Err(TextError::InvalidConstraint { width });
        }
        let width = if width == 0.0 { 0.0 } else { width };
        Ok(Self(width.to_bits()))
    }

    pub(crate) fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShapedTextMetrics {
    width: f32,
    height: f32,
    first_baseline: Option<f32>,
}

impl ShapedTextMetrics {
    pub(crate) fn width(self) -> f32 {
        self.width
    }

    pub(crate) fn height(self) -> f32 {
        self.height
    }

    pub(crate) fn first_baseline(self) -> Option<f32> {
        self.first_baseline
    }
}

/// Retained Parley result shared by Taffy measurement and scene construction.
#[derive(Debug)]
pub(crate) struct ShapedParagraph {
    source: NodeId,
    fingerprint: TextFingerprint,
    constraint: TextConstraint,
    layout: Layout<TextBrush>,
    metrics: ShapedTextMetrics,
}

impl ShapedParagraph {
    pub(crate) fn source(&self) -> NodeId {
        self.source
    }

    pub(crate) fn fingerprint(&self) -> TextFingerprint {
        self.fingerprint
    }

    pub(crate) fn constraint(&self) -> TextConstraint {
        self.constraint
    }

    pub(crate) fn layout(&self) -> &Layout<TextBrush> {
        &self.layout
    }

    pub(crate) fn metrics(&self) -> ShapedTextMetrics {
        self.metrics
    }
}

#[derive(Debug)]
struct WidthVariant {
    constraint: TextConstraint,
    paragraph: Rc<ShapedParagraph>,
    last_used: u64,
}

#[derive(Debug)]
struct SourceEntry {
    input: ParagraphInput,
    font_generation: u64,
    shaping_configuration: u64,
    unbroken: Layout<TextBrush>,
    variants: Vec<WidthVariant>,
}

impl SourceEntry {
    fn matches(&self, input: &ParagraphInput, font_generation: u64) -> bool {
        self.input.fingerprint() == input.fingerprint()
            && self.input == *input
            && self.font_generation == font_generation
            && self.shaping_configuration == SHAPING_CONFIGURATION_VERSION
    }
}

/// Reusable MTS text engine with persistent shaping and bounded width caches.
pub(crate) struct TextEngine {
    font_context: FontContext,
    layout_context: LayoutContext<TextBrush>,
    font_generation: u64,
    cache: HashMap<NodeId, SourceEntry>,
    access_tick: u64,
}

impl std::fmt::Debug for TextEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextEngine")
            .field("font_generation", &self.font_generation)
            .field("cached_sources", &self.cache.len())
            .finish_non_exhaustive()
    }
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// Create an engine backed by the platform font collection.
    pub(crate) fn new() -> Self {
        Self::with_font_context(FontContext::new())
    }

    /// Create an engine with no platform fonts. Intended for deterministic
    /// fixtures that explicitly register a licensed embedded font.
    #[cfg(test)]
    fn without_system_fonts() -> Self {
        Self::with_font_context(FontContext {
            collection: Collection::new(CollectionOptions {
                shared: false,
                system_fonts: false,
            }),
            source_cache: SourceCache::default(),
        })
    }

    fn with_font_context(font_context: FontContext) -> Self {
        Self {
            font_context,
            layout_context: LayoutContext::new(),
            font_generation: 0,
            cache: HashMap::new(),
            access_tick: 0,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.font_generation
    }

    /// Register one OpenType blob. A successful change invalidates every
    /// cached source because it can alter fallback as well as explicit family
    /// resolution.
    pub(crate) fn register_font_data(&mut self, data: Vec<u8>) -> Result<usize, TextError> {
        let registered = self
            .font_context
            .collection
            .register_fonts(Blob::from(data), None);
        if registered.is_empty() {
            return Err(TextError::InvalidFontData);
        }
        let count = registered.iter().map(|(_, fonts)| fonts.len()).sum();
        self.font_generation = self.font_generation.wrapping_add(1);
        self.cache.clear();
        Ok(count)
    }

    pub(crate) fn shape(
        &mut self,
        input: &ParagraphInput,
        constraint: TextConstraint,
    ) -> Result<Rc<ShapedParagraph>, TextError> {
        self.access_tick = self.access_tick.wrapping_add(1);
        let tick = self.access_tick;
        let source = input.source();

        let needs_rebuild = self
            .cache
            .get(&source)
            .is_none_or(|entry| !entry.matches(input, self.font_generation));
        if needs_rebuild {
            let unbroken = self.build_unbroken(input)?;
            self.cache.insert(
                source,
                SourceEntry {
                    input: input.clone(),
                    font_generation: self.font_generation,
                    shaping_configuration: SHAPING_CONFIGURATION_VERSION,
                    unbroken,
                    variants: Vec::new(),
                },
            );
        }

        let entry = self
            .cache
            .get_mut(&source)
            .expect("a source cache entry was inserted above");
        if let Some(variant) = entry
            .variants
            .iter_mut()
            .find(|variant| variant.constraint == constraint)
        {
            variant.last_used = tick;
            return Ok(Rc::clone(&variant.paragraph));
        }

        let paragraph = Rc::new(shape_variant(input, &entry.unbroken, constraint)?);
        if entry.variants.len() == MAX_WIDTH_VARIANTS {
            let oldest = entry
                .variants
                .iter()
                .enumerate()
                .min_by_key(|(_, variant)| variant.last_used)
                .map(|(index, _)| index)
                .expect("a full variant cache is non-empty");
            entry.variants.swap_remove(oldest);
        }
        entry.variants.push(WidthVariant {
            constraint,
            paragraph: Rc::clone(&paragraph),
            last_used: tick,
        });
        Ok(paragraph)
    }

    pub(crate) fn retain_sources(&mut self, sources: &HashSet<NodeId>) {
        self.cache.retain(|source, _| sources.contains(source));
    }

    #[cfg(test)]
    fn cached_variant_count(&self, source: NodeId) -> usize {
        self.cache
            .get(&source)
            .map_or(0, |entry| entry.variants.len())
    }

    fn build_unbroken(&mut self, input: &ParagraphInput) -> Result<Layout<TextBrush>, TextError> {
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, input.text(), 1.0, false);
        for property in style_properties(input.base_style()) {
            builder.push_default(property);
        }
        for run in input.runs() {
            for property in style_properties(run.style()) {
                builder.push(property, run.range());
            }
        }
        Ok(builder.build(input.text()))
    }
}

fn shape_variant(
    input: &ParagraphInput,
    unbroken: &Layout<TextBrush>,
    constraint: TextConstraint,
) -> Result<ShapedParagraph, TextError> {
    let mut layout = unbroken.clone();
    let max_advance = match constraint {
        TextConstraint::MaxContent => None,
        TextConstraint::MinContent => {
            let width = layout.calculate_content_widths().min;
            validate_metric(input.source(), "minimum content width", width)?;
            Some(width.max(0.0))
        }
        TextConstraint::Definite(width) => Some(width.get()),
    };
    layout.break_all_lines(max_advance);
    layout.align(Alignment::Start, AlignmentOptions::default());

    let width = layout.width();
    let height = layout.height();
    validate_metric(input.source(), "width", width)?;
    validate_metric(input.source(), "height", height)?;
    let first_baseline = layout.lines().next().map(|line| line.metrics().baseline);
    if let Some(baseline) = first_baseline {
        validate_metric(input.source(), "first baseline", baseline)?;
    }

    Ok(ShapedParagraph {
        source: input.source(),
        fingerprint: input.fingerprint(),
        constraint,
        layout,
        metrics: ShapedTextMetrics {
            width,
            height,
            first_baseline,
        },
    })
}

fn style_properties(style: &ComputedTextStyle) -> [StyleProperty<'_, TextBrush>; 6] {
    let color = style.color;
    [
        StyleProperty::FontFamily(FontFamily::Source(Cow::Borrowed(&style.font_family))),
        StyleProperty::FontSize(style.font_size),
        StyleProperty::FontWeight(FontWeight::new(f32::from(style.font_weight.get()))),
        StyleProperty::Brush([color.red, color.green, color.blue, color.alpha]),
        StyleProperty::LineHeight(match style.line_height {
            LineHeight::Normal => ParleyLineHeight::MetricsRelative(1.0),
            LineHeight::Factor(value) => ParleyLineHeight::FontSizeRelative(value),
            LineHeight::Length(value) => ParleyLineHeight::Absolute(value),
        }),
        StyleProperty::TextWrapMode(match style.wrap {
            TextWrap::Wrap => TextWrapMode::Wrap,
            TextWrap::NoWrap => TextWrapMode::NoWrap,
        }),
    ]
}

fn validate_metric(source: NodeId, field: &'static str, value: f32) -> Result<(), TextError> {
    if !value.is_finite() || value < 0.0 {
        return Err(TextError::InvalidMetric {
            paragraph: source,
            field,
            value,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use crate::ui::elements::styles::{
        color::RgbaColor,
        text::{ComputedTextStyle, FontWeight},
    };

    use super::*;
    use crate::ui::text::StyledTextRun;

    const TEST_FONT: &[u8] = include_bytes!("../../../testdata/fonts/NotoSans-Regular.ttf");

    fn engine() -> TextEngine {
        let mut engine = TextEngine::without_system_fonts();
        assert_eq!(engine.register_font_data(TEST_FONT.to_vec()).unwrap(), 1);
        engine
    }

    fn input_with_style(
        source: NodeId,
        text: &str,
        style: ComputedTextStyle,
        ranges: Vec<(Range<usize>, ComputedTextStyle)>,
    ) -> ParagraphInput {
        ParagraphInput::new(
            source,
            style.clone(),
            text.into(),
            if ranges.is_empty() && !text.is_empty() {
                vec![StyledTextRun::new(0..text.len(), style)]
            } else {
                ranges
                    .into_iter()
                    .map(|(range, style)| StyledTextRun::new(range, style))
                    .collect()
            },
        )
    }

    fn source() -> NodeId {
        use crate::ui::elements::{Dom, Element, ElementTag};
        let mut dom = Dom::new();
        dom.create_element(Element::from_tag(ElementTag::Text))
    }

    fn test_style() -> ComputedTextStyle {
        ComputedTextStyle {
            font_family: "Noto Sans".into(),
            ..ComputedTextStyle::default()
        }
    }

    #[test]
    fn shapes_max_content_and_wraps_at_a_narrow_definite_width() {
        let source = source();
        let input = input_with_style(source, "one two three four five", test_style(), Vec::new());
        let mut engine = engine();

        let wide = engine.shape(&input, TextConstraint::MaxContent).unwrap();
        let narrow = engine
            .shape(&input, TextConstraint::definite(50.0).unwrap())
            .unwrap();

        assert!(wide.metrics().width() > 50.0);
        assert!(narrow.layout().len() > wide.layout().len());
        assert!(narrow.metrics().height() > wide.metrics().height());
        assert!(wide.metrics().first_baseline().is_some());
    }

    #[test]
    fn nowrap_keeps_one_line_under_a_narrow_constraint() {
        let source = source();
        let mut style = test_style();
        style.wrap = TextWrap::NoWrap;
        let input = input_with_style(source, "one two three four", style, Vec::new());
        let mut engine = engine();

        let shaped = engine
            .shape(&input, TextConstraint::definite(10.0).unwrap())
            .unwrap();

        assert_eq!(shaped.layout().len(), 1);
        assert!(shaped.metrics().width() > 10.0);
    }

    #[test]
    fn inherited_run_properties_reach_parley() {
        let source = source();
        let base = test_style();
        let mut emphasized = base.clone();
        emphasized.font_size = 28.0;
        emphasized.font_weight = FontWeight::BOLD;
        emphasized.color = RgbaColor::rgb(10, 20, 30);
        emphasized.line_height = LineHeight::Factor(1.5);
        let input = input_with_style(
            source,
            "normal bold",
            base,
            vec![(0..7, test_style()), (7..11, emphasized.clone())],
        );
        let mut engine = engine();

        let shaped = engine.shape(&input, TextConstraint::MaxContent).unwrap();
        let runs = shaped
            .layout()
            .lines()
            .flat_map(|line| line.items())
            .filter_map(|item| match item {
                parley::PositionedLayoutItem::GlyphRun(run) => Some(run),
                parley::PositionedLayoutItem::InlineBox(_) => None,
            })
            .collect::<Vec<_>>();

        assert!(runs.iter().any(|run| run.run().font_size() == 28.0));
        assert!(runs
            .iter()
            .any(|run| run.style().brush == [10, 20, 30, 255]));
        assert!(runs
            .iter()
            .any(|run| { (run.run().font_attrs().weight.value() - 700.0).abs() < f32::EPSILON }));
    }

    #[test]
    fn matching_constraints_reuse_retained_variants_and_cache_is_bounded() {
        let source = source();
        let input = input_with_style(source, "cache these words", test_style(), Vec::new());
        let mut engine = engine();

        let first = engine
            .shape(&input, TextConstraint::definite(100.0).unwrap())
            .unwrap();
        let again = engine
            .shape(&input, TextConstraint::definite(100.0).unwrap())
            .unwrap();
        assert!(Rc::ptr_eq(&first, &again));

        for width in [20.0, 30.0, 40.0, 50.0, 60.0] {
            engine
                .shape(&input, TextConstraint::definite(width).unwrap())
                .unwrap();
        }
        assert_eq!(engine.cached_variant_count(source), MAX_WIDTH_VARIANTS);
        let rebuilt = engine
            .shape(&input, TextConstraint::definite(100.0).unwrap())
            .unwrap();
        assert!(!Rc::ptr_eq(&first, &rebuilt));
    }

    #[test]
    fn text_style_and_font_generation_changes_invalidate_the_source() {
        let source = source();
        let input = input_with_style(source, "invalidate", test_style(), Vec::new());
        let mut engine = engine();
        let first = engine.shape(&input, TextConstraint::MaxContent).unwrap();

        let mut changed_style = test_style();
        changed_style.color = RgbaColor::rgb(1, 2, 3);
        let changed = input_with_style(source, "invalidate", changed_style, Vec::new());
        let second = engine.shape(&changed, TextConstraint::MaxContent).unwrap();
        assert!(!Rc::ptr_eq(&first, &second));

        let old_generation = engine.generation();
        engine.register_font_data(TEST_FONT.to_vec()).unwrap();
        assert_eq!(engine.generation(), old_generation + 1);
        let third = engine.shape(&changed, TextConstraint::MaxContent).unwrap();
        assert!(!Rc::ptr_eq(&second, &third));
    }

    #[test]
    fn min_content_is_stable_and_empty_text_uses_parleys_native_metrics() {
        let source_id = source();
        let input = input_with_style(source_id, "longest short", test_style(), Vec::new());
        let empty = input_with_style(source(), "", test_style(), Vec::new());
        let mut engine = engine();

        let min = engine.shape(&input, TextConstraint::MinContent).unwrap();
        let min_again = engine.shape(&input, TextConstraint::MinContent).unwrap();
        let max = engine.shape(&input, TextConstraint::MaxContent).unwrap();
        let empty = engine.shape(&empty, TextConstraint::MaxContent).unwrap();

        assert!(Rc::ptr_eq(&min, &min_again));
        assert!(min.metrics().width() <= max.metrics().width());
        assert!(empty.metrics().width() >= 0.0);
        assert!(empty.metrics().height() >= 0.0);
    }

    #[test]
    fn rejects_invalid_definite_constraints() {
        for width in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(matches!(
                TextConstraint::definite(width),
                Err(TextError::InvalidConstraint { .. })
            ));
        }
        assert_eq!(
            TextConstraint::definite(-0.0).unwrap().definite_value(),
            Some(0.0)
        );
    }
}
