use render::{
    Border, BoxStyle, Canvas, CornerRadius, Rect, TextConstraints, TextSpan,
    TextStyle as RenderTextStyle, TextSystem, TextWidth,
};
use taffy::{AvailableSpace, NodeId, Size};

use super::{
    computed::taffy::{LayoutTree, MeasureElement},
    elements::{
        styles::{shared::background::Background, text::TextStyle},
        Elements,
    },
};

/// Lays out the first window in an app tree and records its drawing commands.
pub fn build_canvas(
    root: &Elements,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> Canvas {
    let mut canvas = Canvas::new().with_clear_color(render::Color::WHITE);
    let Some(window) = first_window(root) else {
        return canvas;
    };

    let width = viewport_width.max(0.0);
    let height = viewport_height.max(0.0);
    let mut layout = LayoutTree::with_measure(window, |element: &Elements, known, available| {
        measure_element(element, known, available, text_system)
    });
    layout.compute_layout(Size {
        width: AvailableSpace::Definite(width),
        height: AvailableSpace::Definite(height),
    });

    paint_node(
        &layout,
        layout.root(),
        [0.0, 0.0],
        scale_factor.max(f32::EPSILON),
        &mut canvas,
    );
    canvas
}

fn first_window(root: &Elements) -> Option<&Elements> {
    let Elements::App { children } = root else {
        return None;
    };
    children
        .iter()
        .find(|child| matches!(child, Elements::Window { .. }))
}

fn measure_element(
    element: &Elements,
    known_dimensions: Size<Option<f32>>,
    available_space: Size<AvailableSpace>,
    text_system: &mut TextSystem,
) -> Size<f32> {
    let Elements::Text { style, .. } = element else {
        return Size {
            width: known_dimensions.width.unwrap_or(0.0),
            height: known_dimensions.height.unwrap_or(0.0),
        };
    };

    let base_style: RenderTextStyle = style.as_ref().clone().into();
    let spans = text_spans(element);
    let width = known_dimensions
        .width
        .map(TextWidth::AtMost)
        .unwrap_or_else(|| match available_space.width {
            AvailableSpace::Definite(width) => TextWidth::AtMost(width),
            AvailableSpace::MinContent => TextWidth::MinContent,
            AvailableSpace::MaxContent => TextWidth::Unconstrained,
        });
    let metrics = text_system
        .layout_rich_metrics(&spans, &base_style, TextConstraints { width })
        .text;

    Size {
        width: known_dimensions.width.unwrap_or(metrics.width),
        height: known_dimensions.height.unwrap_or(metrics.height),
    }
}

fn paint_node<Measure>(
    tree: &LayoutTree<'_, Measure>,
    node: NodeId,
    parent_origin: [f32; 2],
    scale: f32,
    canvas: &mut Canvas,
) where
    Measure: MeasureElement,
{
    let Some(layout) = tree.layout(node) else {
        return;
    };
    let origin = [
        parent_origin[0] + layout.location.x,
        parent_origin[1] + layout.location.y,
    ];
    let rect = Rect::new(
        origin[0] * scale,
        origin[1] * scale,
        layout.size.width * scale,
        layout.size.height * scale,
    );

    match tree.element(node) {
        Some(Elements::Div { style, .. }) => {
            canvas.draw_box(
                rect,
                scaled_box_style(&style.background, style.border, style.corner_radius, scale),
            );
        }
        Some(Elements::Flex { style, .. }) => {
            canvas.draw_box(
                rect,
                scaled_box_style(&style.background, style.border, style.corner_radius, scale),
            );
        }
        Some(Elements::Grid { style, .. }) => {
            canvas.draw_box(
                rect,
                scaled_box_style(&style.background, style.border, style.corner_radius, scale),
            );
        }
        Some(element @ Elements::Text { style, .. }) => {
            let base = scaled_text_style(style.as_ref(), scale);
            let spans = text_spans(element).into_iter().map(|mut span| {
                span.style = scale_render_text_style(span.style, scale);
                span
            });
            canvas.draw_rich_text_with_clips(rect, spans, base, []);
        }
        Some(Elements::App { .. } | Elements::Window { .. } | Elements::_String { .. }) | None => {}
    };

    if let Some(children) = tree.children(node) {
        for child in children {
            paint_node(tree, *child, origin, scale, canvas);
        }
    }
}

fn scaled_box_style(
    background: &Background,
    border: Option<Border>,
    corner_radius: CornerRadius,
    scale: f32,
) -> BoxStyle {
    BoxStyle {
        background: background.color,
        background_image: background.image.clone(),
        border: border.map(|border| Border::new(border.width * scale, border.color)),
        corner_radius: scale_corner_radius(corner_radius, scale),
        ..BoxStyle::default()
    }
}

fn scale_corner_radius(radius: CornerRadius, scale: f32) -> CornerRadius {
    CornerRadius::new(
        radius.top_left * scale,
        radius.top_right * scale,
        radius.bottom_right * scale,
        radius.bottom_left * scale,
    )
}

fn scaled_text_style(style: &TextStyle, scale: f32) -> RenderTextStyle {
    scale_render_text_style(style.clone().into(), scale)
}

fn scale_render_text_style(mut style: RenderTextStyle, scale: f32) -> RenderTextStyle {
    style.font_size *= scale;
    style.line_height *= scale;
    style.letter_spacing *= scale;
    style.word_spacing *= scale;
    style
}

fn text_spans(element: &Elements) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    collect_text_spans(element, &mut spans);
    spans
}

fn collect_text_spans(element: &Elements, spans: &mut Vec<TextSpan>) {
    let Elements::Text { style, children } = element else {
        return;
    };
    let style: RenderTextStyle = style.as_ref().clone().into();
    for child in children {
        match child {
            Elements::_String { string } => {
                spans.push(TextSpan::new(string.clone(), style.clone()))
            }
            nested @ Elements::Text { .. } => collect_text_spans(nested, spans),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use render::{Color, DrawCommand};
    use taffy::FlexDirection;

    use super::*;
    use crate::ui::elements::styles::{div::DivStyle, flex::FlexStyle, text::TextStyle};

    fn text(value: &str, color: Color) -> Elements {
        Elements::Text {
            style: Box::new(TextStyle {
                color,
                ..TextStyle::default()
            }),
            children: vec![Elements::_String {
                string: value.into(),
            }],
        }
    }

    #[test]
    fn paints_boxes_and_nested_text_in_layout_order() {
        let root = Elements::App {
            children: vec![Elements::Window {
                children: vec![Elements::Flex {
                    style: Box::new(FlexStyle {
                        direction: FlexDirection::Column,
                        background: Background {
                            color: Color::from_rgba8(240, 244, 255, 255),
                            image: None,
                        },
                        ..FlexStyle::default()
                    }),
                    children: vec![Elements::Div {
                        style: Box::new(DivStyle::default()),
                        children: vec![text("hello", Color::BLACK)],
                    }],
                }],
            }],
        };

        let canvas = build_canvas(&root, 320.0, 200.0, 1.0, &mut TextSystem::new());

        assert_eq!(canvas.commands().len(), 3);
        assert!(matches!(canvas.commands()[0], DrawCommand::Box { .. }));
        assert!(matches!(canvas.commands()[1], DrawCommand::Box { .. }));
        assert!(matches!(canvas.commands()[2], DrawCommand::Text { .. }));
    }

    #[test]
    fn nested_text_is_emitted_as_one_rich_text_command() {
        let root = Elements::App {
            children: vec![Elements::Window {
                children: vec![Elements::Text {
                    style: Box::new(TextStyle::default()),
                    children: vec![
                        Elements::_String {
                            string: "hello ".into(),
                        },
                        text("world", Color::from_rgba8(20, 80, 200, 255)),
                    ],
                }],
            }],
        };

        let canvas = build_canvas(&root, 320.0, 200.0, 2.0, &mut TextSystem::new());
        let DrawCommand::Text {
            text, spans, style, ..
        } = &canvas.commands()[0]
        else {
            panic!("expected one text command");
        };
        assert_eq!(text, "hello world");
        assert_eq!(spans.len(), 2);
        assert_eq!(style.font_size, TextStyle::default().font_size * 2.0);
    }
}
