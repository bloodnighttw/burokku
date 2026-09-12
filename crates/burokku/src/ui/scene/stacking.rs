use std::collections::HashMap;

use crate::ui::{
    elements::{
        styles::{position::Position, z_index::ZIndex},
        Dom, ElementTag, NodeId,
    },
    layout::ComputedLayout,
};

#[derive(Clone, Copy, Debug)]
struct ContextMeta {
    parent: Option<NodeId>,
    level: i32,
    source_order: usize,
}

#[derive(Debug, Default)]
struct StackingContexts {
    root: Option<NodeId>,
    contexts: HashMap<NodeId, ContextMeta>,
    children: HashMap<NodeId, Vec<NodeId>>,
    positioned_auto: HashMap<NodeId, Vec<NodeId>>,
    source_order: HashMap<NodeId, usize>,
}

impl StackingContexts {
    fn build(dom: &Dom, computed: &ComputedLayout) -> Self {
        let Some(root) = computed.window() else {
            return Self::default();
        };
        let mut tree = Self {
            root: Some(root),
            ..Self::default()
        };
        tree.contexts.insert(
            root,
            ContextMeta {
                parent: None,
                level: 0,
                source_order: 0,
            },
        );
        tree.children.insert(root, Vec::new());
        tree.positioned_auto.insert(root, Vec::new());
        tree.source_order.insert(root, 0);

        let mut source_order = 0;
        let mut pending = dom
            .children(root)
            .into_iter()
            .flatten()
            .rev()
            .map(|&child| (child, root))
            .collect::<Vec<_>>();
        while let Some((node, parent_context)) = pending.pop() {
            if computed.box_for(node).is_none() {
                continue;
            }
            source_order += 1;
            tree.source_order.insert(node, source_order);
            let element = dom.element(node).expect("layout boxes are elements");
            let next_context = if creates_stacking_context(dom, node) {
                tree.contexts.insert(
                    node,
                    ContextMeta {
                        parent: Some(parent_context),
                        level: element.z_index().level(),
                        source_order,
                    },
                );
                tree.children
                    .get_mut(&parent_context)
                    .expect("ancestor stacking context exists")
                    .push(node);
                tree.children.insert(node, Vec::new());
                tree.positioned_auto.insert(node, Vec::new());
                node
            } else {
                if element.position() != Position::Static {
                    tree.positioned_auto
                        .get_mut(&parent_context)
                        .expect("ancestor stacking context exists")
                        .push(node);
                }
                parent_context
            };

            if let Some(children) = dom.children(node) {
                pending.extend(children.iter().rev().map(|&child| (child, next_context)));
            }
        }
        tree
    }

    fn paint_order(&self, dom: &Dom) -> Vec<NodeId> {
        let Some(root) = self.root else {
            return Vec::new();
        };
        let mut order = Vec::with_capacity(self.source_order.len());
        self.paint_context(dom, root, &mut order);
        debug_assert_eq!(order.len(), self.source_order.len());
        order
    }

    fn paint_context(&self, dom: &Dom, root: NodeId, order: &mut Vec<NodeId>) {
        order.push(root);
        let child_contexts = self
            .children
            .get(&root)
            .expect("every stacking context has child storage");
        debug_assert!(child_contexts
            .iter()
            .all(|child| self.contexts[child].parent == Some(root)));

        let mut negative = child_contexts
            .iter()
            .copied()
            .filter(|child| self.contexts[child].level < 0)
            .collect::<Vec<_>>();
        negative.sort_by_key(|child| {
            let meta = self.contexts[child];
            (meta.level, meta.source_order)
        });
        for child in negative {
            self.paint_context(dom, child, order);
        }

        self.paint_normal_children(dom, root, order);

        let mut zero = child_contexts
            .iter()
            .copied()
            .filter(|child| self.contexts[child].level == 0)
            .map(|node| (self.source_order[&node], ZeroItem::Context(node)))
            .chain(
                self.positioned_auto[&root]
                    .iter()
                    .copied()
                    .map(|node| (self.source_order[&node], ZeroItem::Positioned(node))),
            )
            .collect::<Vec<_>>();
        zero.sort_by_key(|(source_order, _)| *source_order);
        for (_, item) in zero {
            match item {
                ZeroItem::Context(node) => self.paint_context(dom, node, order),
                ZeroItem::Positioned(node) => self.paint_positioned(dom, node, order),
            }
        }

        let mut positive = child_contexts
            .iter()
            .copied()
            .filter(|child| self.contexts[child].level > 0)
            .collect::<Vec<_>>();
        positive.sort_by_key(|child| {
            let meta = self.contexts[child];
            (meta.level, meta.source_order)
        });
        for child in positive {
            self.paint_context(dom, child, order);
        }
    }

    fn paint_normal_children(&self, dom: &Dom, parent: NodeId, order: &mut Vec<NodeId>) {
        let Some(children) = dom.children(parent) else {
            return;
        };
        for &child in children {
            if !self.source_order.contains_key(&child)
                || self.contexts.contains_key(&child)
                || dom
                    .element(child)
                    .is_some_and(|element| element.position() != Position::Static)
            {
                continue;
            }
            order.push(child);
            self.paint_normal_children(dom, child, order);
        }
    }

    fn paint_positioned(&self, dom: &Dom, node: NodeId, order: &mut Vec<NodeId>) {
        order.push(node);
        self.paint_normal_children(dom, node, order);
    }
}

#[derive(Clone, Copy)]
enum ZeroItem {
    Context(NodeId),
    Positioned(NodeId),
}

fn creates_stacking_context(dom: &Dom, node: NodeId) -> bool {
    let element = dom.element(node).expect("layout boxes are elements");
    if element.position() == Position::Fixed {
        return true;
    }
    if element.z_index() == ZIndex::Auto {
        return false;
    }
    element.position() != Position::Static
        || dom
            .parent(node)
            .and_then(|parent| dom.element(parent))
            .is_some_and(|parent| matches!(parent.tag(), ElementTag::Flex | ElementTag::Grid))
}

pub(super) fn paint_order(dom: &Dom, computed: &ComputedLayout) -> Vec<NodeId> {
    StackingContexts::build(dom, computed).paint_order(dom)
}

#[cfg(test)]
mod tests {
    use crate::ui::{
        elements::{Dom, Element, ElementTag},
        layout::{LayoutEngine, LogicalViewport},
        text::TextEngine,
    };

    use super::*;

    fn element(dom: &mut Dom, tag: ElementTag) -> NodeId {
        dom.create_element(Element::from_tag(tag))
    }

    fn computed(dom: &Dom) -> LayoutEngine<TextEngine> {
        let mut layout = LayoutEngine::new(TextEngine::without_system_fonts());
        layout
            .compute(dom, LogicalViewport::new(100.0, 100.0).unwrap())
            .unwrap();
        layout
    }

    #[test]
    fn builds_contexts_from_css_position_and_flex_item_rules() {
        // <window>
        //   <div id="ordinary" z-index="9" />
        //   <flex>
        //     <div id="flex-item" z-index="2" />
        //   </flex>
        //   <div position="relative">
        //     <div position="absolute">
        //       <div id="nested" position="relative" z-index="3" />
        //     </div>
        //     <div id="fixed" position="fixed" />
        //   </div>
        // </window>
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let ordinary = element(&mut dom, ElementTag::Div);
        let flex = element(&mut dom, ElementTag::Flex);
        let flex_item = element(&mut dom, ElementTag::Div);
        let relative = element(&mut dom, ElementTag::Div);
        let absolute = element(&mut dom, ElementTag::Div);
        let nested = element(&mut dom, ElementTag::Div);
        let fixed = element(&mut dom, ElementTag::Div);
        for (node, z_index) in [(ordinary, "9"), (flex_item, "2"), (nested, "3")] {
            dom.set_style_property(node, "z-index", z_index).unwrap();
        }
        for (node, position) in [
            (relative, "relative"),
            (absolute, "absolute"),
            (nested, "relative"),
            (fixed, "fixed"),
        ] {
            dom.set_style_property(node, "position", position).unwrap();
        }
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, ordinary).unwrap();
        dom.append_child(window, flex).unwrap();
        dom.append_child(flex, flex_item).unwrap();
        dom.append_child(window, relative).unwrap();
        dom.append_child(relative, absolute).unwrap();
        dom.append_child(absolute, nested).unwrap();
        dom.append_child(relative, fixed).unwrap();
        let layout = computed(&dom);

        let contexts = StackingContexts::build(&dom, layout.current().unwrap());

        assert!(!contexts.contexts.contains_key(&ordinary));
        assert!(!contexts.contexts.contains_key(&relative));
        assert!(!contexts.contexts.contains_key(&absolute));
        for node in [flex_item, nested, fixed] {
            assert_eq!(contexts.contexts[&node].parent, Some(window));
        }
    }

    #[test]
    fn nested_contexts_are_atomic_in_their_parent() {
        // <window>
        //   <div id="first" position="relative" z-index="1">
        //     <div id="inner" position="absolute" z-index="999" />
        //   </div>
        //   <div id="normal" />
        //   <div id="auto" position="absolute" />
        //   <div id="zero" position="relative" z-index="0" />
        //   <div id="second" position="relative" z-index="2" />
        //   <div id="negative" position="relative" z-index="-1" />
        // </window>
        let mut dom = Dom::new();
        let window = element(&mut dom, ElementTag::Window);
        let first = element(&mut dom, ElementTag::Div);
        let inner = element(&mut dom, ElementTag::Div);
        let normal = element(&mut dom, ElementTag::Div);
        let auto = element(&mut dom, ElementTag::Div);
        let zero = element(&mut dom, ElementTag::Div);
        let second = element(&mut dom, ElementTag::Div);
        let negative = element(&mut dom, ElementTag::Div);
        for (node, position, z_index) in [
            (first, "relative", "1"),
            (inner, "absolute", "999"),
            (zero, "relative", "0"),
            (second, "relative", "2"),
            (negative, "relative", "-1"),
        ] {
            dom.set_style_property(node, "position", position).unwrap();
            dom.set_style_property(node, "z-index", z_index).unwrap();
        }
        dom.set_style_property(auto, "position", "absolute")
            .unwrap();
        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, first).unwrap();
        dom.append_child(first, inner).unwrap();
        dom.append_child(window, normal).unwrap();
        dom.append_child(window, auto).unwrap();
        dom.append_child(window, zero).unwrap();
        dom.append_child(window, second).unwrap();
        dom.append_child(window, negative).unwrap();
        let layout = computed(&dom);

        let contexts = StackingContexts::build(&dom, layout.current().unwrap());

        assert_eq!(contexts.contexts[&inner].parent, Some(first));
        assert_eq!(
            contexts.paint_order(&dom),
            vec![window, negative, normal, auto, zero, first, inner, second]
        );
    }
}
