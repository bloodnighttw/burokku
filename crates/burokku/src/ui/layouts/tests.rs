use render::{BoxStyle, TextStyle};

use super::*;

fn text_layout(element_id: u64, x: f32, y: f32) -> Layout {
    Layout {
        element_id,
        x,
        y,
        width: 80.0,
        height: 20.0,
        clips: Vec::new(),
        kind: LayoutKind::Text {
            text: "Burokku".into(),
            style: TextStyle::default(),
        },
    }
}

fn box_layout(element_id: u64, stacking_layer: StackingLayer, children: Vec<Layout>) -> Layout {
    Layout {
        element_id,
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
        clips: Vec::new(),
        kind: LayoutKind::Box {
            style: BoxStyle::default(),
            stacking_layer,
            children,
        },
    }
}

fn element_ids<'a>(layouts: impl Iterator<Item = &'a Layout>) -> Vec<u64> {
    layouts.map(Layout::element_id).collect()
}

#[test]
fn layout_can_be_transferred_through_a_channel() {
    let layout = text_layout(7, 10.0, 20.0);
    let (sender, receiver) = std::sync::mpsc::channel();

    sender.send(layout).unwrap();

    assert_eq!(receiver.recv().unwrap().element_id(), 7);
}

#[test]
fn hit_testing_returns_the_topmost_child() {
    let layout = Layout {
        element_id: 1,
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 200.0,
        clips: Vec::new(),
        kind: LayoutKind::Box {
            style: BoxStyle::default(),
            stacking_layer: StackingLayer::default(),
            children: vec![text_layout(2, 20.0, 20.0), text_layout(3, 20.0, 20.0)],
        },
    };

    assert_eq!(layout.hit_test(30.0, 30.0).unwrap().element_id(), 3);
    assert!(layout.hit_test(300.0, 300.0).is_none());
}

#[test]
fn iterator_uses_stacking_context_render_order() {
    let high_descendant = box_layout(3, StackingLayer::new(Some(10), false), vec![]);
    let ordinary_parent = box_layout(2, StackingLayer::default(), vec![high_descendant]);
    let middle_context = box_layout(4, StackingLayer::new(Some(5), false), vec![]);
    let negative_context = box_layout(5, StackingLayer::new(Some(-1), false), vec![]);
    let root = box_layout(
        1,
        StackingLayer::default(),
        vec![ordinary_parent, middle_context, negative_context],
    );

    assert_eq!(element_ids(root.iter()), [1, 5, 2, 4, 3]);
    assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 3);
}

#[test]
fn isolation_contains_descendant_z_indices() {
    let high_descendant = box_layout(3, StackingLayer::new(Some(10), false), vec![]);
    let isolated_parent = box_layout(2, StackingLayer::new(None, true), vec![high_descendant]);
    let middle_context = box_layout(4, StackingLayer::new(Some(5), false), vec![]);
    let root = box_layout(
        1,
        StackingLayer::default(),
        vec![isolated_parent, middle_context],
    );

    assert_eq!(element_ids(root.iter()), [1, 2, 3, 4]);
    assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 4);
}

#[test]
fn reverse_iterator_is_the_exact_reverse_render_order() {
    let low = box_layout(2, StackingLayer::new(Some(-2), false), vec![]);
    let automatic = box_layout(3, StackingLayer::default(), vec![text_layout(4, 0.0, 0.0)]);
    let high = box_layout(5, StackingLayer::new(Some(8), false), vec![]);
    let root = box_layout(1, StackingLayer::default(), vec![high, automatic, low]);

    let forward = element_ids(root.iter());
    let reverse = element_ids(root.iter_rev());

    assert_eq!(forward, [1, 2, 3, 4, 5]);
    assert_eq!(reverse, [5, 4, 3, 2, 1]);
}
