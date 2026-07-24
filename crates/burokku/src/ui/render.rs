use render::{Border, BoxStyle, Canvas, Color, CornerRadius, Outline, Rect, TextStyle, TextSystem};

use super::{
    elements::Document,
    layouts::{compute_layout, Layout, LayoutKind},
};

/// The computed UI geometry and the drawing commands produced from it.
#[derive(Clone, Debug, PartialEq)]
pub struct UiFrame {
    pub layout: Layout,
    pub canvas: Canvas,
}

/// Computes and preserves the document layout while building drawing commands.
pub fn build_frame(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> UiFrame {
    let layout = compute_layout(
        document,
        viewport_width.max(0.0),
        viewport_height.max(0.0),
        text_system,
    );
    let scale_factor = scale_factor.max(f32::EPSILON);
    let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
    paint_layout(&layout, scale_factor, &mut canvas);
    UiFrame { layout, canvas }
}

/// Computes the document layout and converts it into renderer drawing commands.
pub fn build_canvas(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> Canvas {
    build_frame(
        document,
        viewport_width,
        viewport_height,
        scale_factor,
        text_system,
    )
    .canvas
}

fn paint_layout(layout: &Layout, scale_factor: f32, canvas: &mut Canvas) {
    for layout in layout {
        if layout.width <= 0.0 && layout.height <= 0.0 {
            continue;
        }

        let bounds = Rect::new(
            layout.x * scale_factor,
            layout.y * scale_factor,
            layout.width * scale_factor,
            layout.height * scale_factor,
        );
        match &layout.kind {
            LayoutKind::Box { style, .. } => {
                if *style != BoxStyle::default() {
                    canvas.draw_box(bounds, scaled_box_style(*style, scale_factor));
                }
            }
            LayoutKind::Text { text, style } => {
                canvas.draw_text(bounds, text, scaled_text_style(style, scale_factor));
            }
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
    fn frame_preserves_the_layout_used_to_build_the_canvas() {
        let mut document = Document::new();
        let card = document.create_node(ElementKind::Div);
        document.set_style(card, "width", Some("100px")).unwrap();
        document.set_style(card, "height", Some("50px")).unwrap();
        document
            .set_style(card, "background-color", Some("#ffffff"))
            .unwrap();
        document.insert(BODY_ID, card, None).unwrap();

        let frame = build_frame(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());

        assert_eq!(frame.layout.kind.children()[0].element_id(), card);
        assert_eq!(frame.canvas.commands().len(), 1);
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

    #[test]
    fn canvas_commands_follow_stacking_layer_order() {
        let mut document = Document::new();
        let ordinary = document.create_node(ElementKind::Div);
        let high_descendant = document.create_node(ElementKind::Div);
        let middle = document.create_node(ElementKind::Div);
        let negative = document.create_node(ElementKind::Div);

        for (element, color) in [
            (ordinary, "#ff0000"),
            (high_descendant, "#00ff00"),
            (middle, "#0000ff"),
            (negative, "#000000"),
        ] {
            document
                .set_style(element, "background-color", Some(color))
                .unwrap();
            document.set_style(element, "width", Some("20px")).unwrap();
            document.set_style(element, "height", Some("20px")).unwrap();
        }
        document
            .set_style(high_descendant, "z-index", Some("10"))
            .unwrap();
        document.set_style(middle, "z-index", Some("5")).unwrap();
        document.set_style(negative, "z-index", Some("-1")).unwrap();

        document.insert(BODY_ID, ordinary, None).unwrap();
        document.insert(ordinary, high_descendant, None).unwrap();
        document.insert(BODY_ID, middle, None).unwrap();
        document.insert(BODY_ID, negative, None).unwrap();

        let canvas = build_canvas(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
        let backgrounds: Vec<_> = canvas
            .commands()
            .iter()
            .filter_map(|command| match command {
                render::DrawCommand::Box { style, .. } => Some(style.background),
                render::DrawCommand::Text { .. } => None,
            })
            .collect();

        assert_eq!(
            backgrounds,
            [
                Color::BLACK,
                Color::from_rgba8(0xff, 0, 0, 0xff),
                Color::from_rgba8(0, 0, 0xff, 0xff),
                Color::from_rgba8(0, 0xff, 0, 0xff),
            ]
        );
    }
}
