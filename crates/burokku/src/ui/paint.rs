use render::{Border, BoxStyle, Canvas, Color, CornerRadius, Outline, Rect as RenderRect};
use taffy::prelude::NodeId;

use super::{document::ElementKind, layout::rgba, LayoutError, UiLayout};

impl UiLayout {
    pub fn paint(&self, clear_color: Color) -> Result<Canvas, LayoutError> {
        self.paint_with_scale(clear_color, 1.0)
    }

    pub fn paint_with_scale(
        &self,
        clear_color: Color,
        scale_factor: f32,
    ) -> Result<Canvas, LayoutError> {
        let mut canvas = Canvas::new().with_clear_color(clear_color);
        self.paint_node(
            self.root,
            0.0,
            0.0,
            scale_factor.max(f32::EPSILON),
            &mut canvas,
        )?;
        Ok(canvas)
    }

    fn paint_node(
        &self,
        id: NodeId,
        parent_x: f32,
        parent_y: f32,
        scale_factor: f32,
        canvas: &mut Canvas,
    ) -> Result<(), LayoutError> {
        let layout = self.tree.layout(id)?;
        let x = parent_x + layout.location.x;
        let y = parent_y + layout.location.y;
        let rect = RenderRect::new(
            x * scale_factor,
            y * scale_factor,
            layout.size.width * scale_factor,
            layout.size.height * scale_factor,
        );
        if let Some(data) = self.paint.get(&id) {
            if data.kind == ElementKind::Text {
                if let Some(text) = &data.text {
                    let mut style = text.style.clone();
                    style.font_size *= scale_factor;
                    style.line_height *= scale_factor;
                    canvas.draw_text(rect, &text.text, style);
                }
            } else if data.style.background_color.is_some()
                || data.style.border_width.unwrap_or(0.0) > 0.0
                || data.style.outline_width.unwrap_or(0.0) > 0.0
            {
                canvas.draw_box(
                    rect,
                    BoxStyle {
                        background: data.style.background_color.map_or(Color::TRANSPARENT, rgba),
                        corner_radius: CornerRadius::all(
                            data.style.border_radius.unwrap_or(0.0) * scale_factor,
                        ),
                        border: data
                            .style
                            .border_width
                            .filter(|width| *width > 0.0)
                            .map(|width| {
                                Border::new(
                                    width * scale_factor,
                                    data.style.border_color.map_or(Color::BLACK, rgba),
                                )
                            }),
                        outline: data.style.outline_width.filter(|width| *width > 0.0).map(
                            |width| {
                                Outline::new(
                                    width * scale_factor,
                                    data.style.outline_offset.unwrap_or(0.0) * scale_factor,
                                    data.style.outline_color.map_or(Color::BLACK, rgba),
                                )
                            },
                        ),
                    },
                );
            }
        }
        for child in self.tree.children(id)? {
            self.paint_node(child, x, y, scale_factor, canvas)?;
        }
        Ok(())
    }
}
