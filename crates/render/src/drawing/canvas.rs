use super::{BoxStyle, Clip, Color, Rect, TextStyle};

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Box {
        rect: Rect,
        style: BoxStyle,
        clips: Vec<Clip>,
    },
    Text {
        bounds: Rect,
        text: String,
        style: TextStyle,
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
        self.commands.push(DrawCommand::Text {
            bounds,
            text: text.into(),
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

    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
