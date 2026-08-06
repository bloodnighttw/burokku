use std::iter::FusedIterator;

use super::Elements;

/// A pre-order iterator over an element tree.
///
/// Children that are not valid for their parent element are skipped together
/// with their descendants. Traversal stores one child iterator per ancestor,
/// so its auxiliary memory usage is proportional to the tree's depth rather
/// than its width.
pub struct ElementsIter<'a> {
    root: Option<&'a Elements>,
    ancestors: Vec<Children<'a>>,
}

struct Children<'a> {
    parent: &'a Elements,
    children: std::slice::Iter<'a, Elements>,
    accepted: usize,
}

impl<'a> ElementsIter<'a> {
    pub(super) fn new(root: &'a Elements) -> Self {
        Self {
            root: Some(root),
            ancestors: Vec::new(),
        }
    }
}

impl<'a> Iterator for ElementsIter<'a> {
    type Item = &'a Elements;

    fn next(&mut self) -> Option<Self::Item> {
        let element = if let Some(root) = self.root.take() {
            root
        } else {
            loop {
                let Some(ancestor) = self.ancestors.last_mut() else {
                    return None;
                };
                let parent = ancestor.parent;

                // Multiple windows are not supported yet, so an App's
                // traversal stops after its first valid Window child.
                if matches!(parent, Elements::App { .. }) && ancestor.accepted == 1 {
                    self.ancestors.pop();
                    continue;
                }

                if let Some(child) = ancestor.children.find(|child| accepts_child(parent, child)) {
                    ancestor.accepted += 1;
                    break child;
                }

                self.ancestors.pop();
            }
        };

        if let Some(children) = element.children() {
            self.ancestors.push(Children {
                parent: element,
                children: children.iter(),
                accepted: 0,
            });
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

    #[test]
    fn app_traverses_only_the_first_valid_window() {
        let tree = Elements::App {
            children: vec![
                Elements::Div { children: vec![] },
                Elements::Window {
                    children: vec![Elements::Text { children: vec![] }],
                },
                Elements::Window {
                    children: vec![Elements::Div { children: vec![] }],
                },
            ],
        };

        let elements: Vec<_> = tree.iter().collect();

        assert_eq!(elements.len(), 3);
        assert!(matches!(elements[0], Elements::App { .. }));
        assert!(matches!(elements[1], Elements::Window { .. }));
        assert!(matches!(elements[2], Elements::Text { .. }));
    }
}
