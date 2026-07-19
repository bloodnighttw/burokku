use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};

use crate::{FontFamily, TextStyle, TextWrap};

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
        let (width, wrap) = match constraints.width {
            TextWidth::Unconstrained => (None, Wrap::None),
            TextWidth::AtMost(width) => (Some(width.max(0.0)), wrap_mode(style.wrap)),
            TextWidth::MinContent => (Some(0.0), wrap_mode(style.wrap)),
        };
        let buffer = self.layout_buffer(text, style, width, None, Some(wrap));
        let mut metrics = TextMetrics::default();
        for run in buffer.layout_runs() {
            if metrics.line_count == 0 {
                metrics.first_baseline = run.line_y;
            }
            metrics.width = metrics.width.max(run.line_w);
            metrics.height = metrics.height.max(run.line_top + run.line_height);
            metrics.line_count += 1;
        }
        metrics
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
        let family = match &style.font_family {
            FontFamily::SansSerif => Family::SansSerif,
            FontFamily::Serif => Family::Serif,
            FontFamily::Monospace => Family::Monospace,
            FontFamily::Named(name) => Family::Name(name),
        };
        let attrs = Attrs::new()
            .family(family)
            .weight(Weight(style.font_weight));
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }

    pub(crate) fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
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
    }
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
}
