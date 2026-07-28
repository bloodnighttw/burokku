mod output;
mod paint;
mod scroll;
mod style;
mod text;
mod tree;

use std::collections::HashMap;

use render::{Rect as RenderRect, TextStyle, TextSystem, Transform};
use taffy::{
    compute_root_layout,
    geometry::{Point, Size},
    prelude::{AvailableSpace, Dimension, NodeId},
};

use crate::ui::elements::{styles::Position, Document, BODY_ID};

use super::{Layout, ScrollOffset};
use tree::{add_element, add_viewport, establish_positioning_containing_blocks, ElementLayoutTree};

#[cfg(test)]
use super::LayoutKind;
#[cfg(test)]
use crate::ui::elements::ElementKind;

/// Computes a renderable layout tree from an element document.
///
/// The viewport and all returned geometry are in logical CSS pixels. Text is
/// measured by [`TextSystem`], which uses the same Glyphon shaping engine as
/// the renderer.
pub(super) fn compute_layout(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
) -> Layout {
    compute_layout_with_scroll(
        document,
        viewport_width,
        viewport_height,
        text_system,
        &HashMap::new(),
    )
}

pub(super) fn compute_layout_with_scroll(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
    scroll_offsets: &HashMap<u64, ScrollOffset>,
) -> Layout {
    let viewport = Size {
        width: viewport_width.max(0.0),
        height: viewport_height.max(0.0),
    };
    let mut nodes = Vec::new();
    let body = add_element(&mut nodes, document, BODY_ID, &TextStyle::default());
    let has_out_of_flow = nodes.iter().any(|node| {
        matches!(
            node.paint_style.position,
            Position::Absolute | Position::Fixed
        )
    });
    nodes[body].style.size = Size {
        width: Dimension::length(viewport.width),
        height: Dimension::length(viewport.height),
    };
    let root = add_viewport(&mut nodes, body, viewport);

    let mut tree = ElementLayoutTree {
        nodes,
        viewport_root: root,
        text_system,
        scroll_offsets,
    };
    if has_out_of_flow {
        compute_root_layout(
            &mut tree,
            NodeId::from(root),
            viewport.map(AvailableSpace::Definite),
        );
        establish_positioning_containing_blocks(&mut tree.nodes, root);
        tree.clear_layout_caches();
    }
    compute_root_layout(
        &mut tree,
        NodeId::from(root),
        viewport.map(AvailableSpace::Definite),
    );
    tree.to_layout(
        body,
        Point::ZERO,
        &[],
        RenderRect::new(0.0, 0.0, viewport.width, viewport.height),
        Transform::IDENTITY,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use render::{Color, FontFamily, FontStyle, TextAlign, TextConstraints, TextWrap};

    #[test]
    fn computes_flex_geometry_and_glyphon_text_metrics() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Text("first".into()));
        let second = document.create_node(ElementKind::Text("second".into()));
        document.set_style(row, "display", Some("flex")).unwrap();
        document.set_style(row, "width", Some("300px")).unwrap();
        document.set_style(row, "padding", Some("10px")).unwrap();
        document.set_style(row, "gap", Some("20px")).unwrap();
        document
            .set_style(row, "background-color", Some("#102030"))
            .unwrap();
        document.set_style(row, "font-size", Some("20px")).unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, first, None).unwrap();
        document.insert(row, second, None).unwrap();

        let mut text_system = TextSystem::new();
        let layout = compute_layout(&document, 800.0, 600.0, &mut text_system);
        let LayoutKind::Box { children, .. } = &layout.kind else {
            panic!("body should produce a box layout");
        };
        let LayoutKind::Box {
            style,
            children: row_children,
            ..
        } = &children[0].kind
        else {
            panic!("div should produce a box layout");
        };

        assert_eq!((layout.width, layout.height), (800.0, 600.0));
        assert_eq!(children[0].width, 320.0);
        assert_eq!(children[0].x, 0.0);
        assert_eq!(row_children.len(), 2);
        assert_eq!(row_children[0].x, 10.0);
        assert!(row_children[0].width > 0.0);
        assert_eq!(row_children[1].x, 10.0 + row_children[0].width + 20.0);
        assert_eq!(style.background, Color::from_rgba8(0x10, 0x20, 0x30, 0xff));
        let LayoutKind::Text {
            style: text_style, ..
        } = &row_children[0].kind
        else {
            panic!("text should produce a text layout");
        };
        assert_eq!(text_style.font_size, 20.0);
    }

    #[test]
    fn nested_flex_items_retain_final_text_geometry() {
        let mut document = Document::new();
        let gallery = document.create_node(ElementKind::Div);
        let card = document.create_node(ElementKind::Div);
        let row = document.create_node(ElementKind::Div);
        let large_box = document.create_node(ElementKind::Span);
        let small_box = document.create_node(ElementKind::Span);
        let fixed_width_sibling = document.create_node(ElementKind::Span);
        let large = document.create_node(ElementKind::Text("Baseline".into()));
        let small =
            document.create_node(ElementKind::Text("aligned through Glyphon metrics".into()));
        let sibling = document.create_node(ElementKind::Text(
            "Styled, centered, spaced and decorated text with font fallbacks".into(),
        ));
        document
            .set_style(gallery, "display", Some("flex"))
            .unwrap();
        document
            .set_style(gallery, "flex-direction", Some("column"))
            .unwrap();
        document.set_style(gallery, "width", Some("666px")).unwrap();
        document.set_style(card, "display", Some("flex")).unwrap();
        document
            .set_style(card, "flex-direction", Some("column"))
            .unwrap();
        document.set_style(card, "padding", Some("16px")).unwrap();
        document.set_style(row, "display", Some("flex")).unwrap();
        document.set_style(row, "gap", Some("10px")).unwrap();
        document
            .set_style(large_box, "font-size", Some("30px"))
            .unwrap();
        document
            .set_style(small_box, "font-size", Some("14px"))
            .unwrap();
        document
            .set_style(fixed_width_sibling, "width", Some("610px"))
            .unwrap();
        document.insert(BODY_ID, gallery, None).unwrap();
        document.insert(gallery, card, None).unwrap();
        document.insert(card, row, None).unwrap();
        document.insert(card, fixed_width_sibling, None).unwrap();
        document.insert(row, large_box, None).unwrap();
        document.insert(row, small_box, None).unwrap();
        document.insert(large_box, large, None).unwrap();
        document.insert(small_box, small, None).unwrap();
        document.insert(fixed_width_sibling, sibling, None).unwrap();

        let layout = compute_layout(&document, 800.0, 600.0, &mut TextSystem::new());
        let items = layout.children()[0].children()[0].children()[0].children();
        let small_text = &items[1].children()[0];

        assert_eq!(small_text.width, items[1].width);
        assert!(small_text.height <= items[1].height);
    }

    #[test]
    fn recomputes_normal_line_height_and_inherits_typography() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Text("styled text".into()));
        document
            .set_style(parent, "font-size", Some("30px"))
            .unwrap();
        document
            .set_style(parent, "font-family", Some("Missing Face, serif"))
            .unwrap();
        document
            .set_style(parent, "font-style", Some("oblique"))
            .unwrap();
        document
            .set_style(parent, "text-align", Some("right"))
            .unwrap();
        document
            .set_style(parent, "letter-spacing", Some("2px"))
            .unwrap();
        document
            .set_style(parent, "word-spacing", Some("4px"))
            .unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let text = &layout.children()[0].children()[0];
        let LayoutKind::Text { style, .. } = &text.kind else {
            panic!("child should be text");
        };

        assert_eq!(style.font_size, 30.0);
        assert_eq!(style.line_height, 36.0);
        assert!(style.line_height_is_normal);
        assert_eq!(
            style.font_families,
            vec![
                FontFamily::Named("Missing Face".to_owned()),
                FontFamily::Serif
            ]
        );
        assert_eq!(style.font_style, FontStyle::Oblique);
        assert_eq!(style.text_align, TextAlign::Right);
        assert_eq!(style.letter_spacing, 2.0);
        assert_eq!(style.word_spacing, 4.0);
    }

    #[test]
    fn nested_spans_share_inline_layout_and_keep_individual_styles() {
        let mut document = Document::new();
        let line = document.create_node(ElementKind::Span);
        let leading = document.create_node(ElementKind::Text("Hello  ".into()));
        let emphasized = document.create_node(ElementKind::Span);
        let emphasized_text = document.create_node(ElementKind::Text("world".into()));
        let reactive = document.create_node(ElementKind::Text(" 1".into()));
        document.set_style(line, "width", Some("70px")).unwrap();
        document
            .set_style(line, "overflow-wrap", Some("anywhere"))
            .unwrap();
        document
            .set_style(emphasized, "color", Some("#7c3aed"))
            .unwrap();
        document
            .set_style(emphasized, "font-weight", Some("700"))
            .unwrap();
        document.insert(BODY_ID, line, None).unwrap();
        document.insert(line, leading, None).unwrap();
        document.insert(line, emphasized, None).unwrap();
        document.insert(emphasized, emphasized_text, None).unwrap();
        document.insert(line, reactive, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let inline = &layout.children()[0];
        let LayoutKind::Text {
            text,
            spans,
            line_count,
            ..
        } = &inline.kind
        else {
            panic!("a text-only span tree should become one inline text layout");
        };

        assert_eq!(text, "Hello world 1");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].text, "world");
        assert_eq!(
            spans[1].style.color,
            Color::from_rgba8(0x7c, 0x3a, 0xed, 0xff)
        );
        assert_eq!(spans[1].style.font_weight, 700);
        assert!(*line_count > 1);
        assert!(inline.children().is_empty());

        document.set_text(reactive, " 2".into()).unwrap();
        let updated = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let LayoutKind::Text { text, .. } = &updated.children()[0].kind else {
            panic!("updated inline content should remain one text layout");
        };
        assert_eq!(text, "Hello world 2");
    }

    #[test]
    fn jsx_text_fragments_and_variables_share_a_nowrap_line() {
        let mut document = Document::new();
        let line = document.create_node(ElementKind::Span);
        let prefix = document.create_node(ElementKind::Text("Scroll item ".into()));
        let variable = document.create_node(ElementKind::Text("1".into()));
        let suffix = document.create_node(ElementKind::Text(
            " · drag either thumb or use the mouse wheel".into(),
        ));
        document.set_style(line, "width", Some("180px")).unwrap();
        document
            .set_style(line, "white-space", Some("nowrap"))
            .unwrap();
        document.insert(BODY_ID, line, None).unwrap();
        document.insert(line, prefix, None).unwrap();
        document.insert(line, variable, None).unwrap();
        document.insert(line, suffix, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let LayoutKind::Text {
            text,
            spans,
            style,
            line_count,
            ..
        } = &layout.children()[0].kind
        else {
            panic!("adjacent JSX text fragments should become one inline text layout");
        };

        assert_eq!(
            text,
            "Scroll item 1 · drag either thumb or use the mouse wheel"
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(style.wrap, TextWrap::None);
        assert_eq!(*line_count, 1);

        document.set_text(variable, "2".into()).unwrap();
        let updated = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let LayoutKind::Text {
            text, line_count, ..
        } = &updated.children()[0].kind
        else {
            panic!("the updated variable should remain in the inline text flow");
        };
        assert!(text.starts_with("Scroll item 2"));
        assert_eq!(*line_count, 1);
    }

    #[test]
    fn normalizes_text_according_to_white_space_and_wrapping_styles() {
        let mut document = Document::new();
        let normal_box = document.create_node(ElementKind::Div);
        let pre_box = document.create_node(ElementKind::Div);
        let anywhere_box = document.create_node(ElementKind::Div);
        let normal = document.create_node(ElementKind::Text("  a \n  b  ".into()));
        let pre = document.create_node(ElementKind::Text("  a \n  b  ".into()));
        let anywhere = document.create_node(ElementKind::Text("abcdefghij".into()));
        document
            .set_style(pre_box, "white-space", Some("pre"))
            .unwrap();
        document
            .set_style(anywhere_box, "overflow-wrap", Some("anywhere"))
            .unwrap();
        document.insert(BODY_ID, normal_box, None).unwrap();
        document.insert(BODY_ID, pre_box, None).unwrap();
        document.insert(BODY_ID, anywhere_box, None).unwrap();
        document.insert(normal_box, normal, None).unwrap();
        document.insert(pre_box, pre, None).unwrap();
        document.insert(anywhere_box, anywhere, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let children = layout.children();
        let LayoutKind::Text { text, style, .. } = &children[0].children()[0].kind else {
            panic!("normal should be text");
        };
        assert_eq!(text, "a b");
        assert_eq!(style.wrap, TextWrap::Word);
        let LayoutKind::Text { text, style, .. } = &children[1].children()[0].kind else {
            panic!("pre should be text");
        };
        assert_eq!(text, "  a \n  b  ");
        assert_eq!(style.wrap, TextWrap::None);
        let LayoutKind::Text { style, .. } = &children[2].children()[0].kind else {
            panic!("anywhere should be text");
        };
        assert_eq!(style.wrap, TextWrap::Glyph);
    }

    #[test]
    fn explicit_normal_wrapping_values_override_inherited_aggressive_values() {
        let mut document = Document::new();
        let anywhere = document.create_node(ElementKind::Div);
        let normal_overflow = document.create_node(ElementKind::Div);
        let anywhere_text = document.create_node(ElementKind::Text("abcdefgh".into()));
        document
            .set_style(anywhere, "overflow-wrap", Some("anywhere"))
            .unwrap();
        document
            .set_style(normal_overflow, "overflow-wrap", Some("normal"))
            .unwrap();

        let break_all = document.create_node(ElementKind::Div);
        let normal_break = document.create_node(ElementKind::Div);
        let normal_break_text = document.create_node(ElementKind::Text("abcdefgh".into()));
        document
            .set_style(break_all, "word-break", Some("break-all"))
            .unwrap();
        document
            .set_style(normal_break, "word-break", Some("normal"))
            .unwrap();

        let inherited_break_all = document.create_node(ElementKind::Div);
        let keep_all = document.create_node(ElementKind::Div);
        let keep_all_text = document.create_node(ElementKind::Text("abcdefgh".into()));
        document
            .set_style(inherited_break_all, "word-break", Some("break-all"))
            .unwrap();
        document
            .set_style(keep_all, "word-break", Some("keep-all"))
            .unwrap();

        for (outer, inner, text) in [
            (anywhere, normal_overflow, anywhere_text),
            (break_all, normal_break, normal_break_text),
            (inherited_break_all, keep_all, keep_all_text),
        ] {
            document.insert(BODY_ID, outer, None).unwrap();
            document.insert(outer, inner, None).unwrap();
            document.insert(inner, text, None).unwrap();
        }

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        for outer in layout.children() {
            let text = &outer.children()[0].children()[0];
            let LayoutKind::Text { style, .. } = &text.kind else {
                panic!("nested child should be text");
            };
            assert_eq!(style.wrap, TextWrap::Word);
        }
    }

    #[test]
    fn propagates_glyphon_baselines_for_flex_alignment() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let small_box = document.create_node(ElementKind::Div);
        let large_box = document.create_node(ElementKind::Div);
        let small = document.create_node(ElementKind::Text("small".into()));
        let large = document.create_node(ElementKind::Text("large".into()));
        document.set_style(row, "display", Some("flex")).unwrap();
        document
            .set_style(row, "align-items", Some("baseline"))
            .unwrap();
        document
            .set_style(small_box, "font-size", Some("12px"))
            .unwrap();
        document
            .set_style(large_box, "font-size", Some("32px"))
            .unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, small_box, None).unwrap();
        document.insert(row, large_box, None).unwrap();
        document.insert(small_box, small, None).unwrap();
        document.insert(large_box, large, None).unwrap();

        let mut text_system = TextSystem::new();
        let layout = compute_layout(&document, 300.0, 100.0, &mut text_system);
        let children = layout.children()[0].children();
        let small_layout = &children[0].children()[0];
        let large_layout = &children[1].children()[0];
        let LayoutKind::Text {
            text: small_text,
            style: small_style,
            ..
        } = &small_layout.kind
        else {
            panic!("small should be text");
        };
        let LayoutKind::Text {
            text: large_text,
            style: large_style,
            ..
        } = &large_layout.kind
        else {
            panic!("large should be text");
        };
        let small_baseline = text_system
            .measure(small_text, small_style, TextConstraints::UNCONSTRAINED)
            .first_baseline;
        let large_baseline = text_system
            .measure(large_text, large_style, TextConstraints::UNCONSTRAINED)
            .first_baseline;

        assert!((small_layout.y + small_baseline - large_layout.y - large_baseline).abs() < 0.01);
    }

    #[test]
    fn block_baseline_comes_from_the_first_eligible_descendant() {
        let mut document = Document::new();
        let row = document.create_node(ElementKind::Div);
        let multi = document.create_node(ElementKind::Div);
        let first_box = document.create_node(ElementKind::Div);
        let second_box = document.create_node(ElementKind::Div);
        let reference_box = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Text("first".into()));
        let second = document.create_node(ElementKind::Text("second".into()));
        let reference = document.create_node(ElementKind::Text("reference".into()));
        document.set_style(row, "display", Some("flex")).unwrap();
        document
            .set_style(row, "align-items", Some("baseline"))
            .unwrap();
        document
            .set_style(first_box, "font-size", Some("12px"))
            .unwrap();
        document
            .set_style(second_box, "font-size", Some("24px"))
            .unwrap();
        document
            .set_style(reference_box, "font-size", Some("32px"))
            .unwrap();
        document.insert(BODY_ID, row, None).unwrap();
        document.insert(row, multi, None).unwrap();
        document.insert(row, reference_box, None).unwrap();
        document.insert(multi, first_box, None).unwrap();
        document.insert(multi, second_box, None).unwrap();
        document.insert(first_box, first, None).unwrap();
        document.insert(second_box, second, None).unwrap();
        document.insert(reference_box, reference, None).unwrap();

        let mut text_system = TextSystem::new();
        let layout = compute_layout(&document, 300.0, 150.0, &mut text_system);
        let row_children = layout.children()[0].children();
        let first_layout = &row_children[0].children()[0].children()[0];
        let reference_layout = &row_children[1].children()[0];
        let LayoutKind::Text {
            text: first_text,
            style: first_style,
            ..
        } = &first_layout.kind
        else {
            panic!("first descendant should be text");
        };
        let LayoutKind::Text {
            text: reference_text,
            style: reference_style,
            ..
        } = &reference_layout.kind
        else {
            panic!("reference should be text");
        };
        let first_baseline = text_system
            .measure(first_text, first_style, TextConstraints::UNCONSTRAINED)
            .first_baseline;
        let reference_baseline = text_system
            .measure(
                reference_text,
                reference_style,
                TextConstraints::UNCONSTRAINED,
            )
            .first_baseline;

        assert!(
            (first_layout.y + first_baseline - reference_layout.y - reference_baseline).abs()
                < 0.01
        );
    }

    #[test]
    fn returns_absolute_coordinates_for_nested_boxes() {
        let mut document = Document::new();
        let outer = document.create_node(ElementKind::Div);
        let inner = document.create_node(ElementKind::Div);
        document
            .set_style(outer, "margin-left", Some("30px"))
            .unwrap();
        document
            .set_style(outer, "padding-left", Some("12px"))
            .unwrap();
        document.set_style(inner, "width", Some("50px")).unwrap();
        document.set_style(inner, "height", Some("20px")).unwrap();
        document.insert(BODY_ID, outer, None).unwrap();
        document.insert(outer, inner, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let outer = &layout.kind.children()[0];
        let inner = &outer.kind.children()[0];

        assert_eq!(outer.x, 30.0);
        assert_eq!(inner.x, 42.0);
        assert_eq!((inner.width, inner.height), (50.0, 20.0));
    }

    #[test]
    fn static_boxes_ignore_inset_properties() {
        let mut document = Document::new();
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("position", "static"),
            ("left", "40px"),
            ("top", "30px"),
            ("width", "50px"),
            ("height", "20px"),
        ] {
            document.set_style(first, property, Some(value)).unwrap();
        }
        document.set_style(second, "width", Some("50px")).unwrap();
        document.set_style(second, "height", Some("20px")).unwrap();
        document.insert(BODY_ID, first, None).unwrap();
        document.insert(BODY_ID, second, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let first = &layout.children()[0];
        let second = &layout.children()[1];

        assert_eq!((first.x, first.y), (0.0, 0.0));
        assert_eq!((second.x, second.y), (0.0, 20.0));
    }

    #[test]
    fn absolute_boxes_use_the_nearest_positioned_ancestor() {
        let mut document = Document::new();
        let positioned = document.create_node(ElementKind::Div);
        let static_wrapper = document.create_node(ElementKind::Div);
        let absolute = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("position", "relative"),
            ("width", "300px"),
            ("height", "100px"),
        ] {
            document
                .set_style(positioned, property, Some(value))
                .unwrap();
        }
        for (property, value) in [
            ("margin-left", "50px"),
            ("width", "100px"),
            ("height", "20px"),
        ] {
            document
                .set_style(static_wrapper, property, Some(value))
                .unwrap();
        }
        for (property, value) in [
            ("position", "absolute"),
            ("right", "0px"),
            ("top", "5px"),
            ("width", "10px"),
            ("height", "10px"),
        ] {
            document.set_style(absolute, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, positioned, None).unwrap();
        document.insert(positioned, static_wrapper, None).unwrap();
        document.insert(static_wrapper, absolute, None).unwrap();

        let layout = compute_layout(&document, 400.0, 200.0, &mut TextSystem::new());
        let positioned = &layout.children()[0];
        let static_wrapper = &positioned.children()[0];
        let absolute = &static_wrapper.children()[0];

        assert_eq!((static_wrapper.x, static_wrapper.y), (50.0, 0.0));
        assert_eq!((absolute.x, absolute.y), (290.0, 5.0));
    }

    #[test]
    fn absolute_auto_insets_preserve_their_static_position() {
        let mut document = Document::new();
        let positioned = document.create_node(ElementKind::Div);
        let preceding = document.create_node(ElementKind::Div);
        let static_wrapper = document.create_node(ElementKind::Div);
        let absolute = document.create_node(ElementKind::Div);
        document
            .set_style(positioned, "position", Some("relative"))
            .unwrap();
        document
            .set_style(positioned, "width", Some("300px"))
            .unwrap();
        document
            .set_style(preceding, "height", Some("20px"))
            .unwrap();
        document
            .set_style(static_wrapper, "margin-left", Some("30px"))
            .unwrap();
        document
            .set_style(static_wrapper, "height", Some("40px"))
            .unwrap();
        document
            .set_style(absolute, "position", Some("absolute"))
            .unwrap();
        document.set_style(absolute, "width", Some("10px")).unwrap();
        document
            .set_style(absolute, "height", Some("10px"))
            .unwrap();
        document.insert(BODY_ID, positioned, None).unwrap();
        document.insert(positioned, preceding, None).unwrap();
        document.insert(positioned, static_wrapper, None).unwrap();
        document.insert(static_wrapper, absolute, None).unwrap();

        let layout = compute_layout(&document, 400.0, 200.0, &mut TextSystem::new());
        let absolute = &layout.children()[0].children()[1].children()[0];

        assert_eq!((absolute.x, absolute.y), (30.0, 20.0));
    }

    #[test]
    fn carries_z_index_and_isolation_into_layout() {
        let mut document = Document::new();
        let indexed = document.create_node(ElementKind::Div);
        let isolated = document.create_node(ElementKind::Div);
        document.set_style(indexed, "z-index", Some("-7")).unwrap();
        document
            .set_style(isolated, "isolation", Some("isolate"))
            .unwrap();
        document.insert(BODY_ID, indexed, None).unwrap();
        document.insert(BODY_ID, isolated, None).unwrap();

        let layout = compute_layout(&document, 200.0, 100.0, &mut TextSystem::new());
        let children = layout.children();

        let LayoutKind::Box {
            z_index, isolated, ..
        } = &children[0].kind
        else {
            panic!("indexed child should be a box");
        };
        assert_eq!(*z_index, Some(-7));
        assert!(!isolated);

        let LayoutKind::Box {
            z_index, isolated, ..
        } = &children[1].kind
        else {
            panic!("isolated child should be a box");
        };
        assert_eq!(*z_index, None);
        assert!(*isolated);
    }

    #[test]
    fn fixed_boxes_use_the_viewport_for_layout_and_escape_ancestor_clips() {
        let mut document = Document::new();
        let clipped_parent = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("position", "relative"),
            ("left", "40px"),
            ("top", "30px"),
            ("width", "100px"),
            ("height", "60px"),
            ("overflow", "hidden"),
        ] {
            document
                .set_style(clipped_parent, property, Some(value))
                .unwrap();
        }
        for (property, value) in [
            ("position", "fixed"),
            ("width", "25%"),
            ("height", "20px"),
            ("right", "10px"),
            ("bottom", "15px"),
        ] {
            document.set_style(fixed, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, clipped_parent, None).unwrap();
        document.insert(clipped_parent, fixed, None).unwrap();

        let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let fixed = &layout.children()[0].children()[0];

        assert_eq!((fixed.x, fixed.y), (215.0, 165.0));
        assert_eq!((fixed.width, fixed.height), (75.0, 20.0));
        assert!(fixed.clips.is_empty());
        assert!(fixed.is_fixed_to_viewport());
    }

    #[test]
    fn viewport_fixed_boxes_ignore_body_box_model_and_clipping() {
        let mut document = Document::new();
        let wrapper = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("margin-left", "30px"),
            ("padding", "20px"),
            ("border-width", "5px"),
            ("overflow", "hidden"),
        ] {
            document.set_style(BODY_ID, property, Some(value)).unwrap();
        }
        for (property, value) in [
            ("width", "50px"),
            ("height", "50px"),
            ("overflow", "hidden"),
        ] {
            document.set_style(wrapper, property, Some(value)).unwrap();
        }
        for (property, value) in [
            ("position", "fixed"),
            ("width", "50%"),
            ("height", "20px"),
            ("right", "10px"),
            ("bottom", "15px"),
        ] {
            document.set_style(fixed, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, wrapper, None).unwrap();
        document.insert(wrapper, fixed, None).unwrap();

        let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let fixed = &layout.children()[0].children()[0];

        assert_eq!((fixed.x, fixed.y), (140.0, 165.0));
        assert_eq!((fixed.width, fixed.height), (150.0, 20.0));
        assert!(fixed.clips.is_empty());
        assert!(fixed.is_fixed_to_viewport());
    }

    #[test]
    fn transformed_body_contains_fixed_boxes() {
        let mut document = Document::new();
        let wrapper = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("padding", "20px"),
            ("overflow", "hidden"),
            ("transform", "translateX(0px)"),
        ] {
            document.set_style(BODY_ID, property, Some(value)).unwrap();
        }
        for (property, value) in [
            ("position", "fixed"),
            ("width", "50%"),
            ("height", "20px"),
            ("right", "10px"),
            ("bottom", "15px"),
        ] {
            document.set_style(fixed, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, wrapper, None).unwrap();
        document.insert(wrapper, fixed, None).unwrap();

        let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let fixed = &layout.children()[0].children()[0];
        let LayoutKind::Box {
            fixed_containing_block,
            ..
        } = fixed.kind
        else {
            panic!("fixed element should produce a box");
        };

        assert_eq!((fixed.x, fixed.y), (140.0, 205.0));
        assert_eq!((fixed.width, fixed.height), (150.0, 20.0));
        assert_eq!(fixed_containing_block, Some(BODY_ID));
        assert_eq!(fixed.clips.len(), 1);
        assert!(!fixed.is_fixed_to_viewport());
    }

    #[test]
    fn viewport_fixed_auto_insets_keep_their_static_position() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let preceding = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("margin-top", "50px"),
            ("width", "100px"),
            ("height", "100px"),
        ] {
            document.set_style(parent, property, Some(value)).unwrap();
        }
        document
            .set_style(preceding, "height", Some("20px"))
            .unwrap();
        for (property, value) in [("position", "fixed"), ("width", "10px"), ("height", "10px")] {
            document.set_style(fixed, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, preceding, None).unwrap();
        document.insert(parent, fixed, None).unwrap();

        let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let fixed = &layout.children()[0].children()[1];

        assert_eq!((fixed.x, fixed.y), (0.0, 70.0));
    }

    #[test]
    fn retained_scrolling_does_not_move_viewport_fixed_descendants() {
        let mut document = Document::new();
        let scroller = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("width", "100px"),
            ("height", "50px"),
            ("overflow-y", "scroll"),
        ] {
            document.set_style(scroller, property, Some(value)).unwrap();
        }
        document
            .set_style(content, "height", Some("200px"))
            .unwrap();
        for (property, value) in [
            ("position", "fixed"),
            ("left", "9px"),
            ("top", "8px"),
            ("width", "20px"),
            ("height", "10px"),
        ] {
            document.set_style(fixed, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, scroller, None).unwrap();
        document.insert(scroller, content, None).unwrap();
        document.insert(scroller, fixed, None).unwrap();

        let mut layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let before_content_y = layout.children()[0].children()[0].y;
        let before_fixed = {
            let fixed = &layout.children()[0].children()[1];
            (fixed.x, fixed.y)
        };

        assert!(layout.apply_scroll_offset(scroller, ScrollOffset::new(0.0, 30.0)));

        let scroller = &layout.children()[0];
        assert_eq!(scroller.children()[0].y, before_content_y - 30.0);
        assert_eq!(
            (scroller.children()[1].x, scroller.children()[1].y),
            before_fixed
        );
        assert!(scroller.children()[1].clips.is_empty());

        let rebuilt = compute_layout_with_scroll(
            &document,
            300.0,
            200.0,
            &mut TextSystem::new(),
            &HashMap::from([(scroller.element_id, ScrollOffset::new(0.0, 30.0))]),
        );
        let rebuilt_scroller = &rebuilt.children()[0];
        assert_eq!(
            (
                rebuilt_scroller.children()[0].y,
                rebuilt_scroller.children()[1].x,
                rebuilt_scroller.children()[1].y,
            ),
            (
                scroller.children()[0].y,
                scroller.children()[1].x,
                scroller.children()[1].y,
            )
        );
    }

    #[test]
    fn transformed_ancestors_contain_fixed_boxes() {
        let mut document = Document::new();
        let transformed = document.create_node(ElementKind::Div);
        let intermediate = document.create_node(ElementKind::Div);
        let fixed = document.create_node(ElementKind::Div);
        for (property, value) in [
            ("position", "relative"),
            ("left", "40px"),
            ("top", "30px"),
            ("width", "100px"),
            ("height", "60px"),
            ("overflow", "hidden"),
            ("transform", "translateX(0px)"),
        ] {
            document
                .set_style(transformed, property, Some(value))
                .unwrap();
        }
        for (property, value) in [
            ("position", "relative"),
            ("left", "20px"),
            ("top", "10px"),
            ("width", "40px"),
            ("height", "30px"),
            ("overflow", "hidden"),
        ] {
            document
                .set_style(intermediate, property, Some(value))
                .unwrap();
        }
        for (property, value) in [
            ("position", "fixed"),
            ("width", "25%"),
            ("height", "20px"),
            ("right", "10px"),
            ("bottom", "15px"),
        ] {
            document.set_style(fixed, property, Some(value)).unwrap();
        }
        document.insert(BODY_ID, transformed, None).unwrap();
        document.insert(transformed, intermediate, None).unwrap();
        document.insert(intermediate, fixed, None).unwrap();

        let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
        let fixed = &layout.children()[0].children()[0].children()[0];

        assert_eq!((fixed.x, fixed.y), (105.0, 55.0));
        assert_eq!((fixed.width, fixed.height), (25.0, 20.0));
        assert_eq!(fixed.clips.len(), 2);
        assert!(!fixed.is_fixed_to_viewport());
    }

    #[test]
    fn parent_transform_moves_descendants_clips_and_hit_testing_around_parent_center() {
        let mut document = Document::new();
        let parent = document.create_node(ElementKind::Div);
        let child = document.create_node(ElementKind::Div);
        document.set_style(parent, "width", Some("100px")).unwrap();
        document.set_style(parent, "height", Some("100px")).unwrap();
        document
            .set_style(parent, "transform", Some("rotate(90deg)"))
            .unwrap();
        document
            .set_style(parent, "overflow", Some("hidden"))
            .unwrap();
        document.set_style(child, "width", Some("20px")).unwrap();
        document.set_style(child, "height", Some("10px")).unwrap();
        document.insert(BODY_ID, parent, None).unwrap();
        document.insert(parent, child, None).unwrap();

        let layout = compute_layout(&document, 200.0, 200.0, &mut TextSystem::new());
        let parent = &layout.children()[0];
        let child = &parent.children()[0];

        assert!(child.contains(95.0, 10.0));
        assert!(!child.contains(10.0, 5.0));
        assert_eq!(child.clips.len(), 1);
        assert!(child.clips[0].contains(95.0, 10.0));
        assert!(!child.clips[0].contains(10.0, 120.0));
    }

    #[test]
    fn computes_explicit_grid_tracks_and_named_placement() {
        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let item = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document.set_style(grid, "width", Some("300px")).unwrap();
        document.set_style(grid, "height", Some("100px")).unwrap();
        document
            .set_style(
                grid,
                "grid-template-columns",
                Some("[left] 80px [content] 120px [right]"),
            )
            .unwrap();
        document
            .set_style(grid, "grid-template-rows", Some("40px 60px"))
            .unwrap();
        document
            .set_style(item, "grid-column", Some("content / right"))
            .unwrap();
        document.set_style(item, "grid-row", Some("2")).unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, item, None).unwrap();

        let layout = compute_layout(&document, 400.0, 200.0, &mut TextSystem::new());
        let grid = &layout.children()[0];
        let item = &grid.children()[0];

        assert_eq!((grid.width, grid.height), (300.0, 100.0));
        assert_eq!((item.x, item.y), (80.0, 40.0));
        assert_eq!((item.width, item.height), (120.0, 60.0));
    }

    #[test]
    fn computes_implicit_grid_tracks_with_column_auto_flow() {
        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        let third = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document.set_style(grid, "width", Some("200px")).unwrap();
        document.set_style(grid, "height", Some("60px")).unwrap();
        document
            .set_style(grid, "grid-template-rows", Some("30px 30px"))
            .unwrap();
        document
            .set_style(grid, "grid-auto-columns", Some("40px"))
            .unwrap();
        document
            .set_style(grid, "grid-auto-flow", Some("column"))
            .unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, first, None).unwrap();
        document.insert(grid, second, None).unwrap();
        document.insert(grid, third, None).unwrap();

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();

        assert_eq!(
            children
                .iter()
                .map(|child| (child.x, child.y, child.width, child.height))
                .collect::<Vec<_>>(),
            vec![
                (0.0, 0.0, 40.0, 30.0),
                (0.0, 30.0, 40.0, 30.0),
                (40.0, 0.0, 40.0, 30.0),
            ]
        );
    }

    #[test]
    fn computes_named_grid_template_areas() {
        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let header = document.create_node(ElementKind::Div);
        let main = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document.set_style(grid, "width", Some("300px")).unwrap();
        document.set_style(grid, "height", Some("100px")).unwrap();
        document
            .set_style(grid, "grid-template-columns", Some("100px 200px"))
            .unwrap();
        document
            .set_style(grid, "grid-template-rows", Some("40px 60px"))
            .unwrap();
        document
            .set_style(
                grid,
                "grid-template-areas",
                Some("\"header header\" \"sidebar main\""),
            )
            .unwrap();
        document
            .set_style(header, "grid-area", Some("header"))
            .unwrap();
        document.set_style(main, "grid-area", Some("main")).unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, header, None).unwrap();
        document.insert(grid, main, None).unwrap();

        let layout = compute_layout(&document, 400.0, 200.0, &mut TextSystem::new());
        let children = layout.children()[0].children();

        assert_eq!(
            (
                children[0].x,
                children[0].y,
                children[0].width,
                children[0].height
            ),
            (0.0, 0.0, 300.0, 40.0)
        );
        assert_eq!(
            (
                children[1].x,
                children[1].y,
                children[1].width,
                children[1].height
            ),
            (100.0, 40.0, 200.0, 60.0)
        );
    }

    #[test]
    fn flex_shorthand_controls_growth() {
        let mut document = Document::new();
        let flex = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(flex, "display", Some("flex")).unwrap();
        document.set_style(flex, "width", Some("300px")).unwrap();
        document.set_style(first, "flex", Some("2")).unwrap();
        document.set_style(second, "flex", Some("1")).unwrap();
        document.insert(BODY_ID, flex, None).unwrap();
        document.insert(flex, first, None).unwrap();
        document.insert(flex, second, None).unwrap();

        let layout = compute_layout(&document, 400.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();

        assert_eq!(children[0].width, 200.0);
        assert_eq!(children[1].width, 100.0);
        assert_eq!(children[1].x, 200.0);
    }

    #[test]
    fn order_reorders_flex_items_stably_but_not_block_children() {
        let mut document = Document::new();
        let flex = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        let third = document.create_node(ElementKind::Div);
        document.set_style(flex, "display", Some("flex")).unwrap();
        document.set_style(first, "width", Some("40px")).unwrap();
        document.set_style(second, "width", Some("40px")).unwrap();
        document.set_style(third, "width", Some("40px")).unwrap();
        document.set_style(first, "order", Some("2")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.set_style(third, "order", Some("2")).unwrap();
        document.insert(BODY_ID, flex, None).unwrap();
        document.insert(flex, first, None).unwrap();
        document.insert(flex, second, None).unwrap();
        document.insert(flex, third, None).unwrap();

        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();
        assert_eq!(
            children
                .iter()
                .map(|child| child.element_id)
                .collect::<Vec<_>>(),
            vec![second, first, third]
        );
        assert_eq!(
            children.iter().map(|child| child.x).collect::<Vec<_>>(),
            vec![0.0, 40.0, 80.0]
        );

        let mut document = Document::new();
        let grid = document.create_node(ElementKind::Div);
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(grid, "display", Some("grid")).unwrap();
        document
            .set_style(grid, "grid-template-columns", Some("40px 40px"))
            .unwrap();
        document.set_style(first, "order", Some("1")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.insert(BODY_ID, grid, None).unwrap();
        document.insert(grid, first, None).unwrap();
        document.insert(grid, second, None).unwrap();
        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        let children = layout.children()[0].children();
        assert_eq!(
            children
                .iter()
                .map(|child| (child.element_id, child.x))
                .collect::<Vec<_>>(),
            vec![(second, 0.0), (first, 40.0)]
        );

        let mut document = Document::new();
        let first = document.create_node(ElementKind::Div);
        let second = document.create_node(ElementKind::Div);
        document.set_style(first, "order", Some("2")).unwrap();
        document.set_style(second, "order", Some("-1")).unwrap();
        document.insert(BODY_ID, first, None).unwrap();
        document.insert(BODY_ID, second, None).unwrap();
        let layout = compute_layout(&document, 300.0, 100.0, &mut TextSystem::new());
        assert_eq!(
            layout
                .children()
                .iter()
                .map(|child| child.element_id)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
    }
}
