use render::{Border, BoxStyle, Canvas, Color, CornerRadius, Outline, Rect, TextStyle, TextSystem};

use super::{
    elements::Document,
    layouts::{compute_layout, Layout, LayoutKind},
};

/// Computes the document layout and converts it into renderer drawing commands.
pub fn build_canvas(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> Canvas {
    let layout = compute_layout(
        document,
        viewport_width.max(0.0),
        viewport_height.max(0.0),
        text_system,
    );
    let scale_factor = scale_factor.max(f32::EPSILON);
    let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
    paint_layout(&layout, scale_factor, &mut canvas);
    canvas
}

fn paint_layout(layout: &Layout, scale_factor: f32, canvas: &mut Canvas) {
    if layout.width <= 0.0 && layout.height <= 0.0 {
        return;
    }

    let bounds = Rect::new(
        layout.x * scale_factor,
        layout.y * scale_factor,
        layout.width * scale_factor,
        layout.height * scale_factor,
    );
    match &layout.kind {
        LayoutKind::Box { style, children } => {
            if *style != BoxStyle::default() {
                canvas.draw_box(bounds, scaled_box_style(*style, scale_factor));
            }
            for child in children {
                paint_layout(child, scale_factor, canvas);
            }
        }
        LayoutKind::Text { text, style } => {
            canvas.draw_text(bounds, text, scaled_text_style(style, scale_factor));
        }
    }
}

fn scaled_box_style(style: BoxStyle, scale_factor: f32) -> BoxStyle {
    BoxStyle {
        background: style.background,
        corner_radius: CornerRadius::new(
            style.corner_radius.top_left * scale_factor,
            style.corner_radius.top_right * scale_factor,
            style.corner_radius.bottom_right * scale_factor,
            style.corner_radius.bottom_left * scale_factor,
        ),
        border: style
            .border
            .map(|border| Border::new(border.width * scale_factor, border.color)),
        outline: style.outline.map(|outline| {
            Outline::new(
                outline.width * scale_factor,
                outline.offset * scale_factor,
                outline.color,
            )
        }),
    }
}

fn scaled_text_style(style: &TextStyle, scale_factor: f32) -> TextStyle {
    let mut style = style.clone();
    style.font_size *= scale_factor;
    style.line_height *= scale_factor;
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::{ElementKind, BODY_ID};

    #[test]
    fn builds_canvas_from_computed_ui_layout() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        let text = document.create_node(ElementKind::Text("Hello UI".into()));
        document.set_style(card, "width", Some("300px")).unwrap();
        document
            .set_style(card, "background-color", Some("#f5f7fa"))
            .unwrap();
        document.set_style(card, "color", Some("#102030")).unwrap();
        document.insert(BODY_ID, card, None).unwrap();
        document.insert(card, text, None).unwrap();

        let canvas = build_canvas(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());

        assert_eq!(canvas.commands().len(), 2);
    }

    #[test]
    fn scales_geometry_and_paint_styles_for_physical_pixels() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("100px")).unwrap();
        document.set_style(card, "height", Some("50px")).unwrap();
        document
            .set_style(card, "border-width", Some("2px"))
            .unwrap();
        document
            .set_style(card, "border-radius", Some("4px"))
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let canvas = build_canvas(&document, 800.0, 600.0, 2.0, &mut TextSystem::new());
        let render::DrawCommand::Box { rect, style } = &canvas.commands()[0] else {
            panic!("card should produce a box command");
        };

        assert_eq!((rect.width, rect.height), (208.0, 108.0));
        assert_eq!(style.border.expect("border").width, 4.0);
        assert_eq!(style.corner_radius.top_left, 8.0);
    }
}
