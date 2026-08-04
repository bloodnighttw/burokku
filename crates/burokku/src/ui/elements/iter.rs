use std::iter::FusedIterator;

use super::Elements;

/// A pre-order iterator over an element tree.
///
/// Children that are not valid for their parent element are skipped together
/// with their descendants.
pub struct ElementsIter<'a> {
    pending: Vec<&'a Elements>,
}

impl<'a> ElementsIter<'a> {
    pub(super) fn new(root: &'a Elements) -> Self {
        Self {
            pending: vec![root],
        }
    }
}

impl<'a> Iterator for ElementsIter<'a> {
    type Item = &'a Elements;

    fn next(&mut self) -> Option<Self::Item> {
        let element = self.pending.pop()?;

        if let Some(children) = element.children() {
            self.pending.extend(
                children
                    .iter()
                    .rev()
                    .filter(|child| accepts_child(element, child)),
            );
        }

        Some(element)
    }
}

impl FusedIterator for ElementsIter<'_> {}

fn accepts_child(parent: &Elements, child: &Elements) -> bool {
    match parent {
        Elements::App { .. } => matches!(child, Elements::Window { .. }),
        Elements::Window { .. }
        | Elements::Div { .. }
        | Elements::Flex { .. }
        | Elements::Grid { .. } => matches!(
            child,
            Elements::Div { .. }
                | Elements::Flex { .. }
                | Elements::Grid { .. }
                | Elements::Text { .. }
        ),
        Elements::Text { .. } => {
            matches!(child, Elements::Text { .. } | Elements::_String { .. })
        }
        Elements::_String { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_valid_elements_in_pre_order_and_skips_invalid_subtrees() {
        let tree = Elements::App {
            children: vec![
                Elements::Div {
                    children: vec![Elements::Window { children: vec![] }],
                },
                Elements::Window {
                    children: vec![
                        Elements::_String {
                            string: "invalid window child".into(),
                        },
                        Elements::Text {
                            children: vec![
                                Elements::_String {
                                    string: "valid text child".into(),
                                },
                                Elements::Div { children: vec![] },
                                Elements::Text { children: vec![] },
                            ],
                        },
                        Elements::Window {
                            children: vec![Elements::Div { children: vec![] }],
                        },
                        Elements::Div { children: vec![] },
                    ],
                },
            ],
        };

        let mut elements = tree.iter();

        assert!(matches!(elements.next(), Some(Elements::App { .. })));
        assert!(matches!(elements.next(), Some(Elements::Window { .. })));
        assert!(matches!(elements.next(), Some(Elements::Text { .. })));
        assert!(matches!(elements.next(), Some(Elements::_String { .. })));
        assert!(matches!(elements.next(), Some(Elements::Text { .. })));
        assert!(matches!(elements.next(), Some(Elements::Div { .. })));
        assert!(elements.next().is_none());
        assert!(elements.next().is_none());
    }

    #[test]
    fn borrowed_element_implements_into_iterator() {
        let tree = Elements::Div {
            children: vec![Elements::Text { children: vec![] }],
        };

        assert_eq!((&tree).into_iter().count(), 2);
    }
}
