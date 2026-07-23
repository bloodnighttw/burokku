use std::collections::HashMap;

use render::{
    Border, BoxStyle, Canvas, Color, CornerRadius, FontFamily, Outline, Rect as RenderRect,
    TextConstraints, TextStyle, TextSystem,
};
use taffy::{geometry::Point, prelude::*, TaffyError};
use thiserror::Error;

use crate::ui::elements::styles::{
    Color as DomColor, Display as DomDisplay, LengthPercentageValue, LineHeightValue, MaxSizeValue,
    SizeValue, Style as DomStyle,
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
    Style {
        display: match (kind, style.display) {
            (NodeKind::Comment, _) | (_, DomDisplay::None) => Display::None,
            (_, DomDisplay::Block) => Display::Block,
            (_, DomDisplay::Flex) => Display::Flex,
            (_, DomDisplay::Grid) => Display::Grid,
        },
        box_sizing: style.box_sizing,
        position: style.position,
        overflow: Point {
            x: style.overflow_x,
            y: style.overflow_y,
        },
        inset: Rect {
            left: length_percentage_auto(style.left),
            right: length_percentage_auto(style.right),
            top: length_percentage_auto(style.top),
            bottom: length_percentage_auto(style.bottom),
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
            width: max_dimension(style.max_width),
            height: max_dimension(style.max_height),
        },
        aspect_ratio: style.aspect_ratio,
        margin: Rect {
            left: length_percentage_auto(style.margin_left),
            right: length_percentage_auto(style.margin_right),
            top: length_percentage_auto(style.margin_top),
            bottom: length_percentage_auto(style.margin_bottom),
        },
        padding: Rect {
            left: length_percentage(style.padding_left),
            right: length_percentage(style.padding_right),
            top: length_percentage(style.padding_top),
            bottom: length_percentage(style.padding_bottom),
        },
        border: Rect {
            left: LengthPercentage::length(style.border_left_width.px()),
            right: LengthPercentage::length(style.border_right_width.px()),
            top: LengthPercentage::length(style.border_top_width.px()),
            bottom: LengthPercentage::length(style.border_bottom_width.px()),
        },
        align_content: style.align_content,
        align_items: style.align_items,
        align_self: style.align_self,
        justify_content: style.justify_content,
        gap: Size {
            width: length_percentage(style.column_gap),
            height: length_percentage(style.row_gap),
        },
        flex_direction: style.flex_direction,
        flex_wrap: style.flex_wrap,
        flex_basis: dimension(style.flex_basis),
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        ..Default::default()
    }
}

fn dimension(value: SizeValue) -> Dimension {
    match value {
        SizeValue::Auto => Dimension::AUTO,
        SizeValue::Px(value) => Dimension::length(value),
        SizeValue::Percent(value) => Dimension::percent(value / 100.0),
    }
}

fn max_dimension(value: MaxSizeValue) -> Dimension {
    match value {
        MaxSizeValue::None => Dimension::AUTO,
        MaxSizeValue::Px(value) => Dimension::length(value),
        MaxSizeValue::Percent(value) => Dimension::percent(value / 100.0),
    }
}

fn length_percentage(value: LengthPercentageValue) -> LengthPercentage {
    match value {
        LengthPercentageValue::Px(value) => LengthPercentage::length(value),
        LengthPercentageValue::Percent(value) => LengthPercentage::percent(value / 100.0),
    }
}

fn length_percentage_auto(value: SizeValue) -> LengthPercentageAuto {
    match value {
        SizeValue::Auto => LengthPercentageAuto::AUTO,
        SizeValue::Px(value) => LengthPercentageAuto::length(value),
        SizeValue::Percent(value) => LengthPercentageAuto::percent(value / 100.0),
    }
}

fn merge_text_style(parent: &TextStyle, style: &DomStyle) -> TextStyle {
    let mut merged = parent.clone();
    if let Some(color) = style.color {
        merged.color = rgba(color);
    }
    if let Some(font_size) = style.font_size {
        merged.font_size = match font_size {
            LengthPercentageValue::Px(value) => value,
            LengthPercentageValue::Percent(value) => parent.font_size * value / 100.0,
        };
    }
    if let Some(line_height) = style.line_height {
        merged.line_height = match line_height {
            LineHeightValue::Normal => merged.font_size * 1.2,
            LineHeightValue::Number(value) => merged.font_size * value,
            LineHeightValue::Px(value) => value,
            LineHeightValue::Percent(value) => merged.font_size * value / 100.0,
        };
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
                        corner_radius: CornerRadius::new(
                            painted_radius(data.style.border_top_left_radius, rect, scale_factor),
                            painted_radius(data.style.border_top_right_radius, rect, scale_factor),
                            painted_radius(
                                data.style.border_bottom_right_radius,
                                rect,
                                scale_factor,
                            ),
                            painted_radius(
                                data.style.border_bottom_left_radius,
                                rect,
                                scale_factor,
                            ),
                        ),
                        border: (painted_border_width(&data.style) > 0.0).then(|| {
                            Border::new(
                                painted_border_width(&data.style) * scale_factor,
                                data.style.border_color.map_or(Color::BLACK, rgba),
                            )
                        }),
                        outline: (data.style.outline_width.px() > 0.0).then(|| {
                            Outline::new(
                                data.style.outline_width.px() * scale_factor,
                                data.style.outline_offset.px() * scale_factor,
                                data.style.outline_color.map_or(Color::BLACK, rgba),
                            )
                        }),
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
        || painted_border_width(style) > 0.0
        || style.outline_width.px() > 0.0
}

fn painted_border_width(style: &DomStyle) -> f32 {
    [
        style.border_top_width.px(),
        style.border_right_width.px(),
        style.border_bottom_width.px(),
        style.border_left_width.px(),
    ]
    .into_iter()
    .reduce(f32::max)
    .unwrap_or(0.0)
}

fn painted_radius(value: LengthPercentageValue, rect: RenderRect, scale_factor: f32) -> f32 {
    match value {
        LengthPercentageValue::Px(value) => value * scale_factor,
        LengthPercentageValue::Percent(value) => rect.width.min(rect.height) * value / 100.0,
    }
}

fn rgba(color: DomColor) -> Color {
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
