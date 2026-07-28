use std::iter::FusedIterator;

use super::super::{
    stacking::{descendant_contexts, zero_level_entries, Stacking, ZeroLevelEntry},
    Layout,
};

/// An iterator over layouts from front to back in reverse paint order.
///
/// This is an independent reverse traversal, rather than a collected and
/// reversed forward iterator, so its memory use is bounded by pending
/// traversal work instead of the number of layouts in the complete tree.
#[derive(Debug)]
pub struct ReverseLayoutIter<'a> {
    pending: Vec<Task<'a>>,
}

#[derive(Debug)]
enum Task<'a> {
    Context(&'a Layout),
    PositionedAuto(&'a Layout),
    Middle(&'a Layout),
    MiddleChildren(std::slice::Iter<'a, Layout>),
    Yield(&'a Layout),
}

impl<'a> ReverseLayoutIter<'a> {
    pub(in crate::ui::layouts) fn new(root: &'a Layout) -> Self {
        Self {
            pending: vec![Task::Context(root)],
        }
    }
}

impl<'a> Iterator for ReverseLayoutIter<'a> {
    type Item = &'a Layout;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(task) = self.pending.pop() {
            match task {
                Task::Context(layout) => self.schedule_context(layout),
                Task::PositionedAuto(layout) => {
                    self.pending.push(Task::Yield(layout));
                    self.pending
                        .push(Task::MiddleChildren(layout.children().iter()));
                }
                Task::Middle(layout) => {
                    if layout.establishes_stacking_context() || layout.is_positioned_auto() {
                        continue;
                    }

                    self.pending.push(Task::Yield(layout));
                    self.pending
                        .push(Task::MiddleChildren(layout.children().iter()));
                }
                Task::MiddleChildren(mut children) => {
                    if let Some(child) = children.next_back() {
                        self.pending.push(Task::MiddleChildren(children));
                        self.pending.push(Task::Middle(child));
                    }
                }
                Task::Yield(layout) => return Some(layout),
            }
        }

        None
    }
}

impl FusedIterator for ReverseLayoutIter<'_> {}

impl<'a> ReverseLayoutIter<'a> {
    fn schedule_context(&mut self, context_root: &'a Layout) {
        let contexts = descendant_contexts(context_root);
        let zero_level = zero_level_entries(context_root);

        self.pending.push(Task::Yield(context_root));
        self.pending.extend(
            contexts
                .iter()
                .filter(|layout| layout.stacking_index() < 0)
                .map(|layout| Task::Context(layout)),
        );
        self.pending
            .push(Task::MiddleChildren(context_root.children().iter()));
        self.pending
            .extend(zero_level.iter().map(|entry| match entry {
                ZeroLevelEntry::Context(layout) => Task::Context(layout),
                ZeroLevelEntry::PositionedAuto(layout) => Task::PositionedAuto(layout),
            }));
        self.pending.extend(
            contexts
                .iter()
                .filter(|layout| layout.stacking_index() > 0)
                .map(|layout| Task::Context(layout)),
        );
    }
}
