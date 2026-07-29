use render::{TextSpan, TextStyle};
use taffy::prelude::Display;

use crate::ui::elements::styles::Position;
use crate::ui::elements::{styles::Style as ElementStyle, Document, ElementKind, BODY_ID};

use super::compute::text::merge_text_style;

/// An element lowered into the stable tree used for rendering.
///
/// This tree keeps element/style data separate from Taffy's mutable layout
/// state. Its children are stored in order-modified tree order, which is the
/// source order used by the later stacking/paint-order pass.
pub(super) struct RenderNode {
    pub(super) element_id: u64,
    pub(super) kind: ElementKind,
    pub(super) style: ElementStyle,
    pub(super) text_style: TextStyle,
    pub(super) inline_spans: Option<Vec<TextSpan>>,
    /// Children in CSS order-modified tree order, used as paint source order.
    pub(super) paint_children: Vec<RenderNode>,
}

impl RenderNode {
    pub(super) fn from_document(document: &Document) -> Self {
        Self::from_element(document, BODY_ID, &TextStyle::default())
    }

    pub(super) fn viewport(body: Self) -> Self {
        Self {
            element_id: u64::MAX,
            kind: ElementKind::Body,
            style: ElementStyle::default(),
            text_style: TextStyle::default(),
            inline_spans: None,
            paint_children: vec![body],
        }
    }

    pub(super) fn is_text_flow(&self) -> bool {
        matches!(self.kind, ElementKind::Text(_)) || self.inline_spans.is_some()
    }

    pub(super) fn has_out_of_flow_descendant(&self) -> bool {
        self.paint_children.iter().any(|child| {
            matches!(child.style.position, Position::Absolute | Position::Fixed)
                || child.has_out_of_flow_descendant()
        })
    }

    fn from_element(
        document: &Document,
        element_id: u64,
        inherited_text_style: &TextStyle,
    ) -> Self {
        let element = document
            .node(element_id)
            .expect("element child IDs are validated when inserted");
        let text_style = merge_text_style(inherited_text_style, &element.style);
        let inline_spans = if matches!(element.kind, ElementKind::Span)
            && matches!(element.style.display, Display::Block)
            && should_collect_inline_spans(document, &element.children)
        {
            collect_inline_spans(document, element_id, &text_style)
        } else {
            None
        };
        let mut paint_children = if inline_spans.is_some() {
            Vec::new()
        } else {
            element
                .children
                .iter()
                .map(|child| Self::from_element(document, *child, &text_style))
                .collect::<Vec<_>>()
        };
        if matches!(element.style.display, Display::Flex | Display::Grid) {
            paint_children.sort_by_key(|child| child.style.order);
        }

        Self {
            element_id,
            kind: element.kind.clone(),
            style: element.style.clone(),
            text_style,
            inline_spans,
            paint_children,
        }
    }
}

fn should_collect_inline_spans(document: &Document, children: &[u64]) -> bool {
    let mut fragment_count = 0;
    let mut has_nested_span = false;
    for child_id in children {
        let Ok(child) = document.node(*child_id) else {
            continue;
        };
        match &child.kind {
            ElementKind::Text(text) if !text.is_empty() => fragment_count += 1,
            ElementKind::Span if child.style.display == Display::Block => {
                fragment_count += 1;
                has_nested_span = true;
            }
            _ => {}
        }
    }
    has_nested_span || fragment_count > 1
}

fn collect_inline_spans(
    document: &Document,
    element_id: u64,
    text_style: &TextStyle,
) -> Option<Vec<TextSpan>> {
    let element = document
        .node(element_id)
        .expect("inline span descendants are validated when inserted");
    let mut spans = Vec::new();
    for child_id in &element.children {
        let child = document
            .node(*child_id)
            .expect("inline span descendants are validated when inserted");
        match &child.kind {
            ElementKind::Text(text) => {
                if !text.is_empty() {
                    spans.push(TextSpan::new(text, text_style.clone()));
                }
            }
            ElementKind::Comment(_) => {}
            ElementKind::Span if child.style.display == Display::None => {}
            ElementKind::Span if child.style.display == Display::Block => {
                let child_style = merge_text_style(text_style, &child.style);
                spans.extend(collect_inline_spans(document, *child_id, &child_style)?);
            }
            _ => return None,
        }
    }
    (!spans.is_empty()).then_some(spans)
}

#[cfg(test)]
mod tests {
    use crate::ui::elements::{Document, ElementKind, BODY_ID};

    use super::RenderNode;

    #[test]
    fn flex_children_are_stably_stored_in_order_modified_tree_order() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        let third = document.create_node(ElementKind::Div);
        document.set_style(row, "display", Some("flex")).unwrap();
        document.set_style(first, "order", Some("2")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.set_style(third, "order", Some("2")).unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, first, None).unwrap();
        document.insert(row, second, None).unwrap();
        document.insert(row, third, None).unwrap();

        let render_root = RenderNode::from_document(&document);
        let ordered = &render_root.paint_children[0].paint_children;

        assert_eq!(
            ordered
                .iter()
                .map(|node| node.element_id)
                .collect::<Vec<_>>(),
            vec![second, first, third]
        );
    }

    #[test]
    fn block_children_keep_document_order() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(first, "order", Some("2")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, first, None).unwrap();
        document.insert(parent, second, None).unwrap();

        let render_root = RenderNode::from_document(&document);
        let ordered = &render_root.paint_children[0].paint_children;

        assert_eq!(
            ordered
                .iter()
                .map(|node| node.element_id)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
    }
}
