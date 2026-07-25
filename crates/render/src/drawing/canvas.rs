use super::{BoxStyle, Clip, Color, Rect, TextSpan, TextStyle, Transform};

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Box {
        rect: Rect,
        style: BoxStyle,
        clips: Vec<Clip>,
    },
    OverlayBox {
        rect: Rect,
        style: BoxStyle,
        clips: Vec<Clip>,
    },
    Text {
        bounds: Rect,
        text: String,
        spans: Vec<TextSpan>,
        style: TextStyle,
        clips: Vec<Clip>,
    },
    Group {
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
    pub fn new() -> Self {
        Self {
            clear_color: Color::TRANSPARENT,
            commands: Vec::new(),
        }
    }

    pub fn with_clear_color(mut self, color: Color) -> Self {
        self.clear_color = color;
        self
    }

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

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    pub fn draw_group(
        &mut self,
        canvas: Canvas,
        origin: [f32; 2],
        transform: Transform,
        opacity: f32,
        clips: impl IntoIterator<Item = Clip>,
    ) -> &mut Self {
        self.commands.push(DrawCommand::Group {
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
