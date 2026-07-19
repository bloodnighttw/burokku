use super::{BoxStyle, Color, Rect, TextStyle};

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Box {
        rect: Rect,
        style: BoxStyle,
    },
    Text {
        bounds: Rect,
        text: String,
        style: TextStyle,
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
        self.commands.push(DrawCommand::Box { rect, style });
        self
    }

    pub fn draw_text(
        &mut self,
        bounds: Rect,
        text: impl Into<String>,
        style: TextStyle,
    ) -> &mut Self {
        self.commands.push(DrawCommand::Text {
            bounds,
            text: text.into(),
            style,
        });
        self
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
