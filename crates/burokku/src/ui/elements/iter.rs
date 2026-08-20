use std::iter::FusedIterator;

use super::{Dom, NodeId, NodeKind};

/// A pre-order iterator over the reachable nodes in a [`Dom`].
///
/// Each item includes its stable [`NodeId`], which callers may retain after the
/// borrowed node-kind reference expires. Detached nodes are not visited.
pub struct DomIter<'a> {
    dom: &'a Dom,
    pending: Vec<NodeId>,
}

impl<'a> DomIter<'a> {
    pub(super) fn new(dom: &'a Dom) -> Self {
        Self {
            dom,
            pending: vec![dom.root()],
        }
    }
}

impl<'a> Iterator for DomIter<'a> {
    type Item = (NodeId, &'a NodeKind);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.pending.pop()?;
        let node = self
            .dom
            .node(id)
            .expect("reachable DOM relationships only contain live nodes");
        self.pending.extend(node.children().iter().rev().copied());
        Some((id, node.kind()))
    }
}

impl FusedIterator for DomIter<'_> {}

#[cfg(test)]
mod tests {
    use super::super::{DomError, Element};
    use super::*;

    #[test]
    fn iterates_reachable_nodes_in_pre_order_with_stable_ids() {
        let mut dom = Dom::new();
        let detached = dom.create_element(Element::Div {
            style: Box::default(),
        });
        let window = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let text_element = dom.create_element(Element::Text {
            style: Box::default(),
        });
        let text_node = dom.create_text("content");
        let div = dom.create_element(Element::Div {
            style: Box::default(),
        });

        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, text_element).unwrap();
        dom.append_child(text_element, text_node).unwrap();
        dom.append_child(window, div).unwrap();

        let nodes: Vec<_> = dom.iter().collect();

        assert_eq!(
            nodes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![dom.root(), window, text_element, text_node, div]
        );
        assert!(!nodes.iter().any(|(id, _)| *id == detached));
        assert!(matches!(nodes[0].1, NodeKind::App));
        assert!(matches!(
            nodes[1].1,
            NodeKind::Element(Element::Window { .. })
        ));
        assert!(matches!(
            nodes[2].1,
            NodeKind::Element(Element::Text { .. })
        ));
        assert!(matches!(nodes[3].1, NodeKind::Text(text) if text == "content"));
        assert!(matches!(nodes[4].1, NodeKind::Element(Element::Div { .. })));
    }

    #[test]
    fn borrowed_dom_implements_into_iterator() {
        let dom = Dom::new();

        assert_eq!((&dom).into_iter().count(), 1);
    }

    #[test]
    fn app_can_only_have_one_window() {
        let mut dom = Dom::new();
        let first = dom.create_element(Element::Window {
            style: Box::default(),
        });
        let second = dom.create_element(Element::Window {
            style: Box::default(),
        });
        dom.append_child(dom.root(), first).unwrap();

        assert_eq!(
            dom.append_child(dom.root(), second),
            Err(DomError::AppAlreadyHasWindow)
        );
        assert_eq!(dom.iter().count(), 2);
    }
}
