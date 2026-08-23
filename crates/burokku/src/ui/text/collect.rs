use std::collections::HashSet;

use crate::ui::elements::{
    styles::text::ComputedTextStyle, DomSnapshot, Element, NodeId, NodeKind,
};

use super::{ParagraphInput, StyledTextRun, TextError};

/// Result of collecting one outer paragraph.
///
/// `descendants` contains every nested text element and raw text node consumed
/// by the paragraph leaf, in DOM pre-order. The source itself is not repeated.
#[derive(Clone, Debug)]
pub(crate) struct CollectedParagraph {
    input: ParagraphInput,
    descendants: Vec<NodeId>,
}

impl CollectedParagraph {
    pub(crate) fn into_parts(self) -> (ParagraphInput, Vec<NodeId>) {
        (self.input, self.descendants)
    }
}

struct PendingNode {
    id: NodeId,
    parent: NodeId,
    depth: usize,
    inherited_style: ComputedTextStyle,
}

/// Flatten one outer styled-text subtree into inherited UTF-8 runs.
///
/// Traversal is iterative so script-created nesting cannot overflow the Rust
/// stack. `source_depth` is the source's depth in the containing layout tree;
/// `max_depth` therefore applies consistently across ordinary and text nodes.
pub(crate) fn collect_paragraph(
    snapshot: &DomSnapshot,
    source: NodeId,
    source_depth: usize,
    max_depth: usize,
) -> Result<CollectedParagraph, TextError> {
    let source_style = match snapshot.element(source) {
        Some(Element::Text { style }) => &style.text,
        Some(_) => return Err(TextError::ExpectedParagraph(source)),
        None => return Err(TextError::MissingNode(source)),
    };
    if snapshot
        .parent(source)
        .and_then(|parent| snapshot.element(parent))
        .is_some_and(|parent| matches!(parent, Element::Text { .. }))
    {
        return Err(TextError::ExpectedParagraph(source));
    }

    let base_style = source_style.resolve(None);
    let children = snapshot
        .children(source)
        .ok_or(TextError::MissingNode(source))?;
    let mut pending = children
        .iter()
        .rev()
        .map(|&id| PendingNode {
            id,
            parent: source,
            depth: source_depth + 1,
            inherited_style: base_style.clone(),
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::from([source]);
    let mut descendants = Vec::new();
    let mut text = String::new();
    let mut runs: Vec<StyledTextRun> = Vec::new();

    while let Some(next) = pending.pop() {
        assert!(
            next.depth <= max_depth,
            "paragraph tree exceeds the supported depth of {max_depth} at node {:?}",
            next.id
        );
        if snapshot.parent(next.id) != Some(next.parent) {
            return Err(TextError::InvalidRelationship {
                parent: next.parent,
                child: next.id,
            });
        }
        if !seen.insert(next.id) {
            return Err(TextError::DuplicateNode {
                paragraph: source,
                node: next.id,
            });
        }
        descendants.push(next.id);

        let node = snapshot
            .node(next.id)
            .ok_or(TextError::MissingNode(next.id))?;
        match node.kind() {
            NodeKind::Text(value) => append_run(&mut text, &mut runs, value, next.inherited_style),
            NodeKind::Element(Element::Text { style }) => {
                let computed = style.text.resolve(Some(&next.inherited_style));
                pending.extend(node.children().iter().rev().map(|&child| PendingNode {
                    id: child,
                    parent: next.id,
                    depth: next.depth + 1,
                    inherited_style: computed.clone(),
                }));
            }
            NodeKind::App | NodeKind::Element(_) => {
                return Err(TextError::InvalidParagraphChild {
                    paragraph: source,
                    child: next.id,
                });
            }
        }
    }

    validate_runs(source, &text, &runs)?;
    Ok(CollectedParagraph {
        input: ParagraphInput::new(source, base_style, text, runs),
        descendants,
    })
}

fn append_run(
    text: &mut String,
    runs: &mut Vec<StyledTextRun>,
    value: &str,
    style: ComputedTextStyle,
) {
    if value.is_empty() {
        return;
    }

    let start = text.len();
    text.push_str(value);
    let end = text.len();
    if let Some(last) = runs.last_mut() {
        if last.range().end == start && last.style() == &style {
            last.extend_to(end);
            return;
        }
    }
    runs.push(StyledTextRun::new(start..end, style));
}

fn validate_runs(source: NodeId, text: &str, runs: &[StyledTextRun]) -> Result<(), TextError> {
    if text.is_empty() {
        return if runs.is_empty() {
            Ok(())
        } else {
            Err(TextError::InvalidRunCoverage {
                paragraph: source,
                reason: "empty text has non-empty runs",
            })
        };
    }
    if runs.is_empty() {
        return Err(TextError::InvalidRunCoverage {
            paragraph: source,
            reason: "non-empty text has no runs",
        });
    }

    let mut expected_start = 0;
    for run in runs {
        let range = run.range();
        if range.start != expected_start || range.start >= range.end {
            return Err(TextError::InvalidRunCoverage {
                paragraph: source,
                reason: "runs contain a gap, overlap, or empty range",
            });
        }
        if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
            return Err(TextError::InvalidRunCoverage {
                paragraph: source,
                reason: "run endpoint is not a UTF-8 boundary",
            });
        }
        expected_start = range.end;
    }
    if expected_start != text.len() {
        return Err(TextError::InvalidRunCoverage {
            paragraph: source,
            reason: "runs do not cover the complete text",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::ui::elements::{
        styles::{
            color::RgbaColor,
            text::{FontWeight, TextWrap},
        },
        Dom, DomPublisher, Element, ElementTag, PublishedDom,
    };

    use super::*;

    fn element(dom: &mut Dom, tag: ElementTag) -> NodeId {
        dom.create_element(Element::from_tag(tag))
    }

    fn publication(dom: &Dom) -> Arc<PublishedDom> {
        let (_publisher, reader) = DomPublisher::new(dom, |_| {});
        reader.load()
    }

    fn collect(dom: &Dom, source: NodeId) -> CollectedParagraph {
        let publication = publication(dom);
        collect_paragraph(publication.snapshot(), source, 1, 512).unwrap()
    }

    #[test]
    fn collects_nested_inherited_runs_in_dom_order() {
        let mut dom = Dom::new();
        let paragraph = element(&mut dom, ElementTag::Text);
        let first = dom.create_text("hello ");
        let nested = element(&mut dom, ElementTag::Text);
        let nested_text = dom.create_text("styled");
        let tail = dom.create_text(" tail");
        dom.set_style_property(paragraph, "font-family", "Test Sans")
            .unwrap();
        dom.set_style_property(paragraph, "font-size", "20px")
            .unwrap();
        dom.set_style_property(paragraph, "color", "#010203ff")
            .unwrap();
        dom.set_style_property(nested, "font-weight", "bold")
            .unwrap();
        dom.set_style_property(nested, "text-wrap", "nowrap")
            .unwrap();
        dom.append_child(paragraph, first).unwrap();
        dom.append_child(paragraph, nested).unwrap();
        dom.append_child(nested, nested_text).unwrap();
        dom.append_child(paragraph, tail).unwrap();

        let (input, descendants) = collect(&dom, paragraph).into_parts();

        assert_eq!(input.text(), "hello styled tail");
        assert_eq!(descendants, vec![first, nested, nested_text, tail]);
        assert_eq!(input.runs().len(), 3);
        assert_eq!(input.runs()[0].range(), 0..6);
        assert_eq!(input.runs()[1].range(), 6..12);
        assert_eq!(input.runs()[2].range(), 12..17);
        assert_eq!(input.base_style().font_family, "Test Sans");
        assert_eq!(input.base_style().font_size, 20.0);
        assert_eq!(input.runs()[0].style().font_weight, FontWeight::NORMAL);
        assert_eq!(input.runs()[1].style().font_weight, FontWeight::BOLD);
        assert_eq!(input.runs()[1].style().wrap, TextWrap::NoWrap);
        assert_eq!(input.runs()[2].style().color, RgbaColor::rgb(1, 2, 3));
    }

    #[test]
    fn merges_adjacent_equal_styles_across_nodes_and_spans() {
        let mut dom = Dom::new();
        let paragraph = element(&mut dom, ElementTag::Text);
        let first = dom.create_text("a");
        let nested = element(&mut dom, ElementTag::Text);
        let second = dom.create_text("b");
        let empty = dom.create_text("");
        let third = dom.create_text("c");
        dom.append_child(paragraph, first).unwrap();
        dom.append_child(paragraph, nested).unwrap();
        dom.append_child(nested, second).unwrap();
        dom.append_child(nested, empty).unwrap();
        dom.append_child(paragraph, third).unwrap();

        let (input, _) = collect(&dom, paragraph).into_parts();

        assert_eq!(input.text(), "abc");
        assert_eq!(input.runs().len(), 1);
        assert_eq!(input.runs()[0].range(), 0..3);
    }

    #[test]
    fn run_ranges_are_utf8_byte_boundaries() {
        let mut dom = Dom::new();
        let paragraph = element(&mut dom, ElementTag::Text);
        let first = dom.create_text("e\u{301}🙂");
        let nested = element(&mut dom, ElementTag::Text);
        let second = dom.create_text("שלום 世界");
        dom.set_style_property(nested, "color", "#ff0000").unwrap();
        dom.append_child(paragraph, first).unwrap();
        dom.append_child(paragraph, nested).unwrap();
        dom.append_child(nested, second).unwrap();

        let (input, _) = collect(&dom, paragraph).into_parts();

        assert_eq!(input.runs().len(), 2);
        for run in input.runs() {
            let range = run.range();
            assert!(input.text().is_char_boundary(range.start));
            assert!(input.text().is_char_boundary(range.end));
        }
        assert_eq!(input.runs().last().unwrap().range().end, input.text().len());
    }

    #[test]
    fn fingerprints_change_with_inherited_style_but_ignore_span_box_style() {
        let mut dom = Dom::new();
        let paragraph = element(&mut dom, ElementTag::Text);
        let first_parent = element(&mut dom, ElementTag::Text);
        let second_parent = element(&mut dom, ElementTag::Text);
        let moved = element(&mut dom, ElementTag::Text);
        let text = dom.create_text("move me");
        dom.set_style_property(first_parent, "font-size", "12px")
            .unwrap();
        dom.set_style_property(second_parent, "font-size", "30px")
            .unwrap();
        dom.append_child(paragraph, first_parent).unwrap();
        dom.append_child(paragraph, second_parent).unwrap();
        dom.append_child(first_parent, moved).unwrap();
        dom.append_child(moved, text).unwrap();

        let before = collect(&dom, paragraph).into_parts().0;
        dom.append_child(second_parent, moved).unwrap();
        let after_move = collect(&dom, paragraph).into_parts().0;
        assert_ne!(before.fingerprint(), after_move.fingerprint());
        assert_eq!(after_move.runs()[0].style().font_size, 30.0);

        dom.set_style_property(moved, "width", "200px").unwrap();
        let after_box_style = collect(&dom, paragraph).into_parts().0;
        assert_eq!(after_move, after_box_style);
    }

    #[test]
    fn empty_paragraph_retains_its_base_style() {
        let mut dom = Dom::new();
        let paragraph = element(&mut dom, ElementTag::Text);
        dom.set_style_property(paragraph, "font-size", "24px")
            .unwrap();

        let (input, descendants) = collect(&dom, paragraph).into_parts();

        assert!(input.text().is_empty());
        assert!(input.runs().is_empty());
        assert!(descendants.is_empty());
        assert_eq!(input.base_style().font_size, 24.0);
        assert_ne!(input.fingerprint().get(), 0);
    }

    #[test]
    #[should_panic(expected = "paragraph tree exceeds the supported depth")]
    fn nesting_beyond_the_layout_depth_limit_panics_without_recursing() {
        let mut dom = Dom::new();
        let paragraph = element(&mut dom, ElementTag::Text);
        let mut parent = paragraph;
        for _ in 0..8 {
            let child = element(&mut dom, ElementTag::Text);
            dom.append_child(parent, child).unwrap();
            parent = child;
        }
        let publication = publication(&dom);

        let _ = collect_paragraph(publication.snapshot(), paragraph, 1, 4);
    }
}
