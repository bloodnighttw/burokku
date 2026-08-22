use std::ops::Range;

use crate::ui::elements::{
    styles::{
        color::RgbaColor,
        text::{ComputedTextStyle, LineHeight, TextWrap},
    },
    NodeId,
};

/// Stable fingerprint of every text and typography input for one paragraph.
///
/// Cache users must still compare the complete [`ParagraphInput`] after a
/// fingerprint hit; the fingerprint is an accelerator rather than proof of
/// equality.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TextFingerprint(u64);

impl TextFingerprint {
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

/// One complete UTF-8 byte range with fully inherited typography.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StyledTextRun {
    range: Range<usize>,
    style: ComputedTextStyle,
}

impl StyledTextRun {
    pub(crate) fn new(range: Range<usize>, style: ComputedTextStyle) -> Self {
        Self { range, style }
    }

    pub(crate) fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub(crate) fn style(&self) -> &ComputedTextStyle {
        &self.style
    }

    pub(super) fn extend_to(&mut self, end: usize) {
        self.range.end = end;
    }
}

/// Owned shaping input for one outer `<text>` element.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParagraphInput {
    source: NodeId,
    base_style: ComputedTextStyle,
    text: String,
    runs: Vec<StyledTextRun>,
    fingerprint: TextFingerprint,
}

impl ParagraphInput {
    pub(super) fn new(
        source: NodeId,
        base_style: ComputedTextStyle,
        text: String,
        runs: Vec<StyledTextRun>,
    ) -> Self {
        let fingerprint = fingerprint(&base_style, &text, &runs);
        Self {
            source,
            base_style,
            text,
            runs,
            fingerprint,
        }
    }

    pub(crate) fn source(&self) -> NodeId {
        self.source
    }

    pub(crate) fn base_style(&self) -> &ComputedTextStyle {
        &self.base_style
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn runs(&self) -> &[StyledTextRun] {
        &self.runs
    }

    pub(crate) fn fingerprint(&self) -> TextFingerprint {
        self.fingerprint
    }
}

fn fingerprint(
    base_style: &ComputedTextStyle,
    text: &str,
    runs: &[StyledTextRun],
) -> TextFingerprint {
    let mut state = StableHasher::new();
    state.bytes(b"burokku-paragraph-v1");
    hash_style(&mut state, base_style);
    state.usize(text.len());
    state.bytes(text.as_bytes());
    state.usize(runs.len());
    for run in runs {
        state.usize(run.range.start);
        state.usize(run.range.end);
        hash_style(&mut state, &run.style);
    }
    TextFingerprint(state.finish())
}

fn hash_style(state: &mut StableHasher, style: &ComputedTextStyle) {
    state.usize(style.font_family.len());
    state.bytes(style.font_family.as_bytes());
    state.f32(style.font_size);
    state.u16(style.font_weight.get());
    hash_color(state, style.color);
    match style.line_height {
        LineHeight::Normal => state.byte(0),
        LineHeight::Factor(value) => {
            state.byte(1);
            state.f32(value);
        }
        LineHeight::Length(value) => {
            state.byte(2);
            state.f32(value);
        }
    }
    state.byte(match style.wrap {
        TextWrap::Wrap => 0,
        TextWrap::NoWrap => 1,
    });
}

fn hash_color(state: &mut StableHasher, color: RgbaColor) {
    state.byte(color.red);
    state.byte(color.green);
    state.byte(color.blue);
    state.byte(color.alpha);
}

/// Fixed FNV-1a implementation so fingerprints are reproducible across
/// processes and do not depend on `HashMap`'s randomized state.
struct StableHasher(u64);

impl StableHasher {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn byte(&mut self, value: u8) {
        self.0 ^= u64::from(value);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, values: &[u8]) {
        for &value in values {
            self.byte(value);
        }
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.bytes(&(value as u64).to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        let value = if value == 0.0 { 0.0 } else { value };
        self.bytes(&value.to_bits().to_le_bytes());
    }

    const fn finish(self) -> u64 {
        self.0
    }
}
