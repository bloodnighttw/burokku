use std::iter::FusedIterator;

use super::{Dom, Elements, NodeId};

/// A pre-order iterator over the reachable elements in a [`Dom`].
///
/// Each item includes its stable [`NodeId`], which callers may retain after the
/// borrowed element reference expires.
pub struct ElementsIter<'a> {
    dom: &'a Dom,
    pending: Vec<NodeId>,
}

impl<'a> ElementsIter<'a> {
    pub(super) fn new(dom: &'a Dom) -> Self {
        Self {
            dom,
            pending: vec![dom.root()],
        }
    }
}

impl<'a> Iterator for ElementsIter<'a> {
    type Item = (NodeId, &'a Elements);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.pending.pop()?;
        let node = self
            .dom
            .node(id)
            .expect("reachable DOM relationships only contain live nodes");
        self.pending.extend(node.children().iter().rev().copied());
        Some((id, node.element()))
    }
}

impl FusedIterator for ElementsIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_reachable_elements_in_pre_order_with_stable_ids() {
        let mut dom = Dom::new();
        let detached = dom.create(Elements::Div {
            style: Box::default(),
        });
        let window = dom.create(Elements::Window {
            style: Box::default(),
        });
        let text = dom.create(Elements::Text {
            style: Box::default(),
        });
        let string = dom.create(Elements::_String {
            string: "content".into(),
        });
        let div = dom.create(Elements::Div {
            style: Box::default(),
        });

        dom.append_child(dom.root(), window).unwrap();
        dom.append_child(window, text).unwrap();
        dom.append_child(text, string).unwrap();
        dom.append_child(window, div).unwrap();

        let elements: Vec<_> = dom.iter().collect();

        assert_eq!(
            elements.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![dom.root(), window, text, string, div]
        );
        assert!(!elements.iter().any(|(id, _)| *id == detached));
        assert!(matches!(elements[0].1, Elements::App));
        assert!(matches!(elements[1].1, Elements::Window { .. }));
        assert!(matches!(elements[2].1, Elements::Text { .. }));
        assert!(matches!(elements[3].1, Elements::_String { .. }));
        assert!(matches!(elements[4].1, Elements::Div { .. }));
    }

    #[test]
    fn borrowed_dom_implements_into_iterator() {
        let dom = Dom::new();

        assert_eq!((&dom).into_iter().count(), 1);
    }

    #[test]
    fn app_can_only_have_one_window() {
        let mut dom = Dom::new();
        let first = dom.create(Elements::Window {
            style: Box::default(),
        });
        let second = dom.create(Elements::Window {
            style: Box::default(),
        });
        dom.append_child(dom.root(), first).unwrap();

        assert_eq!(
            dom.append_child(dom.root(), second),
            Err(super::super::DomError::AppAlreadyHasWindow)
        );
        assert_eq!(dom.iter().count(), 2);
    }
}
