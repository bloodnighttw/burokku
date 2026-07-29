//! Retained drawing commands recorded by [`Canvas`].
//!
//! Callers can use the convenience box commands for a complete box, or emit
//! individual [`BoxDecoration`] commands when backgrounds, borders, shadows,
//! and outlines need independent paint ordering.

use super::{
    BoxDecoration, BoxStyle, Clip, Color, DecorationStyle, Rect, TextSpan, TextStyle, Transform,
};

/// Coarse paint stages shared by shapes and atomic groups.
///
/// The renderer visits layers from back to front and retains insertion order
/// among commands handled by the same drawing pipeline. Callers should use
/// distinct layers whenever ordering must cross shape, group, or text
/// pipelines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PaintLayer {
    /// Background and border of the element that owns a stacking context.
    ContextBackground,
    /// Atomic stacking contexts whose stack level is negative.
    Negative,
    /// In-flow block decorations such as backgrounds and borders.
    #[default]
    Block,
    /// Inline content and text.
    Content,
    /// Positioned-auto, zero-level, and positive stacking contexts.
    Positioned,
    /// Scrollbars painted above ordinary positioned content.
    Scrollbar,
    /// Viewport-fixed stacking contexts painted above root scrollbars.
    Fixed,
    /// Outlines painted after the stacking context's contents.
    Outline,
    /// UI explicitly drawn above all document content.
    Overlay,
}

impl PaintLayer {
    pub(crate) const COUNT: usize = 9;
    pub const ALL: [Self; 9] = [
        Self::ContextBackground,
        Self::Negative,
        Self::Block,
        Self::Content,
        Self::Positioned,
        Self::Scrollbar,
        Self::Fixed,
        Self::Outline,
        Self::Overlay,
    ];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// One retained operation consumed by the GPU renderer.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    /// A single independently ordered part of a box.
    Decoration {
        layer: PaintLayer,
        rect: Rect,
        decoration: BoxDecoration,
        style: DecorationStyle,
        clips: Vec<Clip>,
    },
    /// Compatibility command that draws every property in one [`BoxStyle`].
    Box {
        rect: Rect,
        style: BoxStyle,
        clips: Vec<Clip>,
    },
    /// Compatibility box command forced into [`PaintLayer::Overlay`].
    OverlayBox {
        rect: Rect,
        style: BoxStyle,
        clips: Vec<Clip>,
    },
    /// Shaped text drawn in [`PaintLayer::Content`].
    Text {
        bounds: Rect,
        text: String,
        spans: Vec<TextSpan>,
        style: TextStyle,
        clips: Vec<Clip>,
    },
    /// An atomic child canvas composited into one paint layer.
    ///
    /// Groups are used for stacking-context isolation and for effects that
    /// must apply to a completed subtree, such as opacity and transforms.
    Group {
        layer: PaintLayer,
        canvas: Box<Canvas>,
        origin: [f32; 2],
        transform: Transform,
        opacity: f32,
        clips: Vec<Clip>,
    },
}

/// Drawing commands for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    pub clear_color: Color,
    commands: Vec<DrawCommand>,
}

impl Canvas {
    /// Creates an empty transparent command list.
    pub fn new() -> Self {
        Self {
            clear_color: Color::TRANSPARENT,
            commands: Vec::new(),
        }
    }

    /// Sets the color used to clear the render target before drawing.
    pub fn with_clear_color(mut self, color: Color) -> Self {
        self.clear_color = color;
        self
    }

    /// Records a complete box using the compatibility [`BoxStyle`] API.
    pub fn draw_box(&mut self, rect: Rect, style: BoxStyle) -> &mut Self {
        self.draw_box_with_clips(rect, style, [])
    }

    pub fn draw_box_with_clips(
        &mut self,
        rect: Rect,
        style: BoxStyle,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        self.commands.push(DrawCommand::Box {
            rect,
            style,
            clips: clips.into_iter().collect(),
        });
        self
    }

    pub fn draw_box_clipped(&mut self, rect: Rect, style: BoxStyle, clip: Clip) -> &mut Self {
        self.draw_box_with_clips(rect, style, [clip])
    }

    /// Records one box decoration in an explicit paint layer.
    ///
    /// Use this instead of [`Self::draw_box`] when parts of a box need to be
    /// separated by other content—for example, a background below text and an
    /// outline above positioned descendants.
    pub fn draw_decoration(
        &mut self,
        layer: PaintLayer,
        rect: Rect,
        decoration: BoxDecoration,
        style: DecorationStyle,
    ) -> &mut Self {
        self.draw_decoration_with_clips(layer, rect, decoration, style, [])
    }

    /// Records one clipped box decoration in an explicit paint layer.
    pub fn draw_decoration_with_clips(
        &mut self,
        layer: PaintLayer,
        rect: Rect,
        decoration: BoxDecoration,
        style: DecorationStyle,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        self.commands.push(DrawCommand::Decoration {
            layer,
            rect,
            decoration,
            style,
            clips: clips.into_iter().collect(),
        });
        self
    }

    pub fn draw_overlay_box_with_clips(
        &mut self,
        rect: Rect,
        style: BoxStyle,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        self.commands.push(DrawCommand::OverlayBox {
            rect,
            style,
            clips: clips.into_iter().collect(),
        });
        self
    }

    pub fn draw_text(
        &mut self,
        bounds: Rect,
        text: impl Into<String>,
        style: TextStyle,
    ) -> &mut Self {
        self.draw_text_with_clips(bounds, text, style, [])
    }

    pub fn draw_text_with_clips(
        &mut self,
        bounds: Rect,
        text: impl Into<String>,
        style: TextStyle,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        let text = text.into();
        self.draw_rich_text_with_clips(bounds, [TextSpan::new(text, style.clone())], style, clips)
    }

    pub fn draw_rich_text_with_clips(
        &mut self,
        bounds: Rect,
        spans: impl IntoIterator<Item = TextSpan>,
        style: TextStyle,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        let spans: Vec<_> = spans.into_iter().collect();
        let text = spans.iter().map(|span| span.text.as_str()).collect();
        self.commands.push(DrawCommand::Text {
            bounds,
            text,
            spans,
            style,
            clips: clips.into_iter().collect(),
        });
        self
    }

    pub fn draw_text_clipped(
        &mut self,
        bounds: Rect,
        text: impl Into<String>,
        style: TextStyle,
        clip: Clip,
    ) -> &mut Self {
        self.draw_text_with_clips(bounds, text, style, [clip])
    }

    /// Returns the retained commands in submission order.
    ///
    /// Final rendering is primarily ordered by [`PaintLayer`], then by the
    /// relevant shape, group, or text pipeline.
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Records an atomic child canvas in the content layer.
    pub fn draw_group(
        &mut self,
        canvas: Canvas,
        origin: [f32; 2],
        transform: Transform,
        opacity: f32,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        self.draw_group_in_layer(
            PaintLayer::Content,
            canvas,
            origin,
            transform,
            opacity,
            clips,
        )
    }

    /// Records an atomic child canvas in an explicit paint layer.
    ///
    /// The child is rendered to a transparent target first. Its transform,
    /// opacity, and clips are then applied once while compositing that target.
    pub fn draw_group_in_layer(
        &mut self,
        layer: PaintLayer,
        canvas: Canvas,
        origin: [f32; 2],
        transform: Transform,
        opacity: f32,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        self.commands.push(DrawCommand::Group {
            layer,
            canvas: Box::new(canvas),
            origin,
            transform,
            opacity: opacity.clamp(0.0, 1.0),
            clips: clips.into_iter().collect(),
        });
        self
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Border, CornerRadius};

    #[test]
    fn stores_independent_decorations_and_group_layers() {
        let mut nested = Canvas::new();
        nested.draw_text(
            Rect::new(0.0, 0.0, 20.0, 10.0),
            "layer",
            TextStyle::default(),
        );

        let mut canvas = Canvas::new();
        canvas.draw_decoration(
            PaintLayer::ContextBackground,
            Rect::new(0.0, 0.0, 40.0, 20.0),
            BoxDecoration::Background {
                color: Color::WHITE,
                image: None,
            },
            DecorationStyle {
                corner_radius: CornerRadius::all(4.0),
                ..DecorationStyle::default()
            },
        );
        canvas.draw_decoration(
            PaintLayer::Block,
            Rect::new(0.0, 0.0, 40.0, 20.0),
            BoxDecoration::Border(Border::new(2.0, Color::BLACK)),
            DecorationStyle::default(),
        );
        canvas.draw_group_in_layer(
            PaintLayer::Positioned,
            nested,
            [20.0, 10.0],
            Transform::IDENTITY,
            1.0,
            [],
        );

        assert!(matches!(
            canvas.commands()[0],
            DrawCommand::Decoration {
                layer: PaintLayer::ContextBackground,
                decoration: BoxDecoration::Background { .. },
                ..
            }
        ));
        assert!(matches!(
            canvas.commands()[1],
            DrawCommand::Decoration {
                layer: PaintLayer::Block,
                decoration: BoxDecoration::Border(_),
                ..
            }
        ));
        assert!(matches!(
            canvas.commands()[2],
            DrawCommand::Group {
                layer: PaintLayer::Positioned,
                ..
            }
        ));
    }
}
