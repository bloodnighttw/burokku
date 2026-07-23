use std::collections::HashMap;

use render::{
    Border, BoxStyle, Canvas, Color, CornerRadius, FontFamily, Outline, Rect as RenderRect,
    TextConstraints, TextStyle, TextSystem,
};
use taffy::{prelude::*, TaffyError};
use thiserror::Error;

use crate::ui::elements::styles::{
    Display as DomDisplay, FlexDirection as DomFlexDirection, Style as DomStyle,
};

use super::{document::Node, Document, NodeKind};

#[derive(Debug, Error)]
pub enum DomRenderError {
    #[error(transparent)]
    Layout(#[from] TaffyError),
}

#[derive(Clone)]
struct TextData {
    text: String,
    style: TextStyle,
}

#[derive(Clone)]
struct PaintData {
    kind: NodeKind,
    style: DomStyle,
    text: Option<TextData>,
}

pub fn build_canvas(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
    text_system: &mut TextSystem,
) -> Result<Canvas, DomRenderError> {
    let mut tree = TaffyTree::new();
    let mut paint = HashMap::new();
    let root = build_node(
        &mut tree,
        &mut paint,
        document,
        document.body(),
        &TextStyle::default(),
    )?;
    let mut root_style = tree.style(root)?.clone();
    root_style.size = Size {
        width: Dimension::length(viewport_width.max(0.0)),
        height: Dimension::length(viewport_height.max(0.0)),
    };
    tree.set_style(root, root_style)?;
    tree.compute_layout_with_measure(
        root,
        Size {
            width: AvailableSpace::Definite(viewport_width.max(0.0)),
            height: AvailableSpace::Definite(viewport_height.max(0.0)),
        },
        |known, available, _id, context, _style| {
            let Some(context) = context else {
                return Size::ZERO;
            };
            let constraints = if let Some(width) = known.width {
                TextConstraints::at_most(width)
            } else {
                match available.width {
                    AvailableSpace::Definite(width) => TextConstraints::at_most(width),
                    AvailableSpace::MinContent => TextConstraints::MIN_CONTENT,
                    AvailableSpace::MaxContent => TextConstraints::UNCONSTRAINED,
                }
            };
            let measured = text_system.measure(&context.text, &context.style, constraints);
            Size {
                width: known.width.unwrap_or(measured.width),
                height: known.height.unwrap_or(measured.height),
            }
        },
    )?;

    let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
    paint_node(
        &tree,
        &paint,
        root,
        0.0,
        0.0,
        scale_factor.max(f32::EPSILON),
        &mut canvas,
    )?;
    Ok(canvas)
}

fn build_node(
    tree: &mut TaffyTree<TextData>,
    paint: &mut HashMap<NodeId, PaintData>,
    document: &Document,
    node: &Node,
    inherited_text: &TextStyle,
) -> Result<NodeId, TaffyError> {
    let text_style = merge_text_style(inherited_text, &node.style);
    let layout_style = to_taffy_style(&node.kind, &node.style);
    let (id, text) = match node.kind {
        NodeKind::Text => {
            let context = TextData {
                text: node.text.clone(),
                style: text_style,
            };
            (
                tree.new_leaf_with_context(layout_style, context.clone())?,
                Some(context),
            )
        }
        NodeKind::Comment => (tree.new_leaf(layout_style)?, None),
        _ => {
            let children = node
                .children
                .iter()
                .map(|child| {
                    build_node(
                        tree,
                        paint,
                        document,
                        document
                            .node(*child)
                            .expect("DOM child IDs are validated when inserted"),
                        &text_style,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            (tree.new_with_children(layout_style, &children)?, None)
        }
    };
    paint.insert(
        id,
        PaintData {
            kind: node.kind.clone(),
            style: node.style.clone(),
            text,
        },
    );
    Ok(id)
}

fn to_taffy_style(kind: &NodeKind, style: &DomStyle) -> Style {
    let default_display = match kind {
        NodeKind::Span | NodeKind::Button => Display::Flex,
        NodeKind::Comment => Display::None,
        _ => Display::Block,
    };
    let dimension = |value: Option<f32>| value.map_or(Dimension::AUTO, Dimension::length);
    let length = |value: Option<f32>| LengthPercentage::length(value.unwrap_or(0.0));
    Style {
        display: match style.display {
            Some(DomDisplay::Block) => Display::Block,
            Some(DomDisplay::Flex) => Display::Flex,
            Some(DomDisplay::None) => Display::None,
            None => default_display,
        },
        flex_direction: match style.flex_direction {
            Some(DomFlexDirection::Column) => FlexDirection::Column,
            Some(DomFlexDirection::Row) | None => FlexDirection::Row,
        },
        size: Size {
            width: dimension(style.width),
            height: dimension(style.height),
        },
        min_size: Size {
            width: dimension(style.min_width),
            height: dimension(style.min_height),
        },
        max_size: Size {
            width: dimension(style.max_width),
            height: dimension(style.max_height),
        },
        flex_grow: style.flex_grow.unwrap_or(0.0),
        flex_shrink: style.flex_shrink.unwrap_or(1.0),
        gap: Size {
            width: length(style.gap),
            height: length(style.gap),
        },
        padding: Rect {
            left: length(style.padding),
            right: length(style.padding),
            top: length(style.padding),
            bottom: length(style.padding),
        },
        margin: Rect {
            left: LengthPercentageAuto::length(style.margin.unwrap_or(0.0)),
            right: LengthPercentageAuto::length(style.margin.unwrap_or(0.0)),
            top: LengthPercentageAuto::length(style.margin.unwrap_or(0.0)),
            bottom: LengthPercentageAuto::length(style.margin.unwrap_or(0.0)),
        },
        border: Rect {
            left: length(style.border_width),
            right: length(style.border_width),
            top: length(style.border_width),
            bottom: length(style.border_width),
        },
        ..Default::default()
    }
}

fn merge_text_style(parent: &TextStyle, style: &DomStyle) -> TextStyle {
    let mut merged = parent.clone();
    if let Some(color) = style.color {
        merged.color = rgba(color);
    }
    if let Some(font_size) = style.font_size {
        merged.font_size = font_size;
    }
    if let Some(line_height) = style.line_height {
        merged.line_height = line_height;
    }
    if let Some(font_weight) = style.font_weight {
        merged.font_weight = font_weight;
    }
    if let Some(font_family) = &style.font_family {
        merged.font_family = FontFamily::Named(font_family.clone());
    }
    merged
}

fn paint_node(
    tree: &TaffyTree<TextData>,
    paint: &HashMap<NodeId, PaintData>,
    id: NodeId,
    parent_x: f32,
    parent_y: f32,
    scale_factor: f32,
    canvas: &mut Canvas,
) -> Result<(), TaffyError> {
    let layout = tree.layout(id)?;
    let x = parent_x + layout.location.x;
    let y = parent_y + layout.location.y;
    let rect = RenderRect::new(
        x * scale_factor,
        y * scale_factor,
        layout.size.width * scale_factor,
        layout.size.height * scale_factor,
    );
    if let Some(data) = paint.get(&id) {
        match &data.kind {
            NodeKind::Text => {
                if let Some(text) = &data.text {
                    let mut style = text.style.clone();
                    style.font_size *= scale_factor;
                    style.line_height *= scale_factor;
                    canvas.draw_text(rect, &text.text, style);
                }
            }
            NodeKind::Comment => {}
            _ if has_box_style(&data.style) => {
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
            _ => {}
        }
    }
    for child in tree.children(id)? {
        paint_node(tree, paint, child, x, y, scale_factor, canvas)?;
    }
    Ok(())
}

fn has_box_style(style: &DomStyle) -> bool {
    style.background_color.is_some()
        || style.border_width.unwrap_or(0.0) > 0.0
        || style.outline_width.unwrap_or(0.0) > 0.0
}

fn rgba(color: [u8; 4]) -> Color {
    Color::from_rgba8(color[0], color[1], color[2], color[3])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{document::BODY_ID, NodeKind};

    #[test]
    fn lays_out_and_paints_a_dom_tree() {
        let mut document = Document::new();
        let card = document.create_node(NodeKind::Div);
        let text = document.create_node(NodeKind::Text);
        document.set_text(text, "Hello DOM".into()).unwrap();
        document.set_style(card, "width", Some("300px")).unwrap();
        document
            .set_style(card, "background-color", Some("#f5f7fa"))
            .unwrap();
        document
            .set_style(text, "color", Some("#102030"))
            .unwrap_err();
        document.insert(BODY_ID, card, None).unwrap();
        document.insert(card, text, None).unwrap();

        let mut text_system = TextSystem::new();
        let canvas = build_canvas(&document, 800.0, 600.0, 1.0, &mut text_system).unwrap();
        assert_eq!(canvas.commands().len(), 2);
    }
}
