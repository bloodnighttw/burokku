use std::collections::HashMap;

use render::{FontFamily, TextConstraints, TextStyle, TextSystem};
use taffy::prelude::*;
use taffy::TaffyError;
use thiserror::Error;

use super::document::{
    Display as UiDisplay, ElementKind, FlexDirection as UiFlexDirection, UiDocument, UiNode,
    UiStyle,
};

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error(transparent)]
    Taffy(#[from] TaffyError),
}

#[derive(Clone)]
pub(super) struct TextContext {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Clone)]
pub(super) struct PaintData {
    pub _id: u64,
    pub kind: ElementKind,
    pub style: UiStyle,
    pub text: Option<TextContext>,
}

pub struct UiLayout {
    pub(super) tree: TaffyTree<TextContext>,
    pub(super) root: NodeId,
    pub(super) paint: HashMap<NodeId, PaintData>,
}

impl UiLayout {
    pub fn compute(
        document: &UiDocument,
        viewport_width: f32,
        viewport_height: f32,
        text_system: &mut TextSystem,
    ) -> Result<Self, LayoutError> {
        let mut tree = TaffyTree::new();
        let mut paint = HashMap::new();
        let inherited = TextStyle::default();
        let root = build_node(&mut tree, &mut paint, &document.root, &inherited)?;
        if document.root.id == 0 {
            let mut root_style = tree.style(root)?.clone();
            root_style.size = Size {
                width: Dimension::from_length(viewport_width.max(0.0)),
                height: Dimension::from_length(viewport_height.max(0.0)),
            };
            tree.set_style(root, root_style)?;
        }
        tree.compute_layout_with_measure(
            root,
            Size {
                width: AvailableSpace::Definite(viewport_width.max(0.0)),
                height: AvailableSpace::Definite(viewport_height.max(0.0)),
            },
            |known, available, _, context, _| {
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
        Ok(Self { tree, root, paint })
    }

    pub fn root_size(&self) -> Result<Size<f32>, LayoutError> {
        Ok(self.tree.layout(self.root)?.size)
    }
}

fn build_node(
    tree: &mut TaffyTree<TextContext>,
    paint: &mut HashMap<NodeId, PaintData>,
    node: &UiNode,
    inherited_text: &TextStyle,
) -> Result<NodeId, TaffyError> {
    let text_style = merge_text_style(inherited_text, &node.style);
    let taffy_style = to_taffy_style(node.kind, &node.style);
    let (id, text) = if node.kind == ElementKind::Text {
        let context = TextContext {
            text: node.text.clone(),
            style: text_style,
        };
        (
            tree.new_leaf_with_context(taffy_style, context.clone())?,
            Some(context),
        )
    } else {
        let children = node
            .children
            .iter()
            .map(|child| build_node(tree, paint, child, &text_style))
            .collect::<Result<Vec<_>, _>>()?;
        (tree.new_with_children(taffy_style, &children)?, None)
    };
    paint.insert(
        id,
        PaintData {
            _id: node.id,
            kind: node.kind,
            style: node.style.clone(),
            text,
        },
    );
    Ok(id)
}

fn to_taffy_style(kind: ElementKind, style: &UiStyle) -> Style {
    let default_display = match kind {
        ElementKind::Div => Display::Block,
        ElementKind::Button | ElementKind::Span => Display::Flex,
        ElementKind::Text => Display::Block,
    };
    let dimension = |value: Option<f32>| value.map_or(Dimension::AUTO, Dimension::from_length);
    let length = |value: Option<f32>| LengthPercentage::from_length(value.unwrap_or(0.0));
    Style {
        display: match style.display {
            Some(UiDisplay::Block) => Display::Block,
            Some(UiDisplay::Flex) => Display::Flex,
            Some(UiDisplay::None) => Display::None,
            None => default_display,
        },
        flex_direction: match style.flex_direction {
            Some(UiFlexDirection::Column) => FlexDirection::Column,
            Some(UiFlexDirection::Row) | None => FlexDirection::Row,
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
            left: LengthPercentageAuto::from_length(style.margin.unwrap_or(0.0)),
            right: LengthPercentageAuto::from_length(style.margin.unwrap_or(0.0)),
            top: LengthPercentageAuto::from_length(style.margin.unwrap_or(0.0)),
            bottom: LengthPercentageAuto::from_length(style.margin.unwrap_or(0.0)),
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

fn merge_text_style(parent: &TextStyle, style: &UiStyle) -> TextStyle {
    let mut merged = parent.clone();
    if let Some(color) = style.color {
        merged.color = rgba(color);
    }
    if let Some(size) = style.font_size {
        merged.font_size = size;
    }
    if let Some(height) = style.line_height {
        merged.line_height = height;
    }
    if let Some(weight) = style.font_weight {
        merged.font_weight = weight;
    }
    if let Some(family) = &style.font_family {
        merged.font_family = FontFamily::Named(family.clone());
    }
    merged
}

pub(super) fn rgba(color: [u8; 4]) -> render::Color {
    render::Color::from_rgba8(color[0], color[1], color[2], color[3])
}
