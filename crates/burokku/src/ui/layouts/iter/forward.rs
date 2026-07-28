use std::iter::FusedIterator;

use super::super::{
    stacking::{descendant_contexts, zero_level_entries, Stacking, ZeroLevelEntry},
    Layout,
};

/// An iterator over layouts from back to front in paint order.
///
/// The iterator walks stacking contexts lazily. It only keeps pending
/// traversal work and the stacking contexts for the layer currently being
/// entered; it does not flatten the layout tree into a second collection.
#[derive(Debug)]
pub struct LayoutIter<'a> {
    pending: Vec<Task<'a>>,
}

#[derive(Debug)]
enum Task<'a> {
    /// Enter an atomic stacking context, yielding its root before its layers.
    Context(&'a Layout),
    /// Paint a positioned `z-index: auto` subtree in the zero-level phase
    /// without containing descendant stacking contexts.
    PositionedAuto(&'a Layout),
    /// Visit an ordinary layout in the middle, in-flow paint phase.
    Middle(&'a Layout),
    /// Continue visiting ordinary children from first to last.
    MiddleChildren(std::slice::Iter<'a, Layout>),
}

impl<'a> LayoutIter<'a> {
    pub(in crate::ui::layouts) fn new(root: &'a Layout) -> Self {
        Self {
            pending: vec![Task::Context(root)],
        }
    }
}

impl<'a> Iterator for LayoutIter<'a> {
    type Item = &'a Layout;

    fn next(&mut self) -> Option<Self::Item> {
        // Tasks form an explicit depth-first traversal stack. Each branch
        // schedules the work that must follow the returned layout, allowing
        // the iterator to yield one layout without flattening the whole tree.
        while let Some(task) = self.pending.pop() {
            match task {
                Task::Context(layout) => {
                    self.schedule_context(layout);
                    return Some(layout);
                }
                Task::PositionedAuto(layout) => {
                    self.pending
                        .push(Task::MiddleChildren(layout.children().iter()));
                    return Some(layout);
                }
                Task::Middle(layout) => {
                    if layout.establishes_stacking_context() || layout.is_positioned_auto() {
                        continue;
                    }

                    self.pending
                        .push(Task::MiddleChildren(layout.children().iter()));
                    return Some(layout);
                }
                Task::MiddleChildren(mut children) => {
                    if let Some(child) = children.next() {
                        self.pending.push(Task::MiddleChildren(children));
                        self.pending.push(Task::Middle(child));
                    }
                }
            }
        }

        None
    }
}

impl FusedIterator for LayoutIter<'_> {}

impl<'a> LayoutIter<'a> {
    fn schedule_context(&mut self, context_root: &'a Layout) {
        let contexts = descendant_contexts(context_root);
        let zero_level = zero_level_entries(context_root);

        // `pending` is LIFO, so phases are pushed from frontmost to backmost.
        // They are consequently visited as negative contexts, ordinary
        // content, zero-level entries, and finally positive contexts.
        self.pending.extend(
            contexts
                .iter()
                .rev()
                .filter(|layout| layout.stacking_index() > 0)
                .map(|layout| Task::Context(layout)),
        );
        self.pending
            .extend(zero_level.iter().rev().map(|entry| match entry {
                ZeroLevelEntry::Context(layout) => Task::Context(layout),
                ZeroLevelEntry::PositionedAuto(layout) => Task::PositionedAuto(layout),
            }));
        self.pending
            .push(Task::MiddleChildren(context_root.children().iter()));
        self.pending.extend(
            contexts
                .iter()
                .rev()
                .filter(|layout| layout.stacking_index() < 0)
                .map(|layout| Task::Context(layout)),
        );
    }
}
