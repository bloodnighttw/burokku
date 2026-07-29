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
    /// Enter an atomic stacking context and schedule its layers front to back.
    Context(&'a Layout),
    /// Visit a positioned `z-index: auto` subtree without containing its
    /// descendant stacking contexts.
    PositionedAuto(&'a Layout),
    /// Visit the ordinary contents of a flex/grid item atomically.
    FlexOrGridItem(&'a Layout),
    /// Visit an ordinary layout in one in-flow paint phase.
    Middle(&'a Layout, MiddlePhase),
    /// Continue visiting ordinary children from last to first.
    MiddleChildren(std::slice::Iter<'a, Layout>, MiddlePhase),
    /// Yield a layout after all content painted above it has been visited.
    Yield(&'a Layout),
}

#[derive(Clone, Copy, Debug)]
enum MiddlePhase {
    Box,
    Content,
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
        // This state machine mirrors the forward traversal without collecting
        // it first. Children and higher stack levels are scheduled before
        // `Yield`, so visually frontmost layouts are returned first.
        while let Some(task) = self.pending.pop() {
            match task {
                Task::Context(layout) => self.schedule_context(layout),
                Task::PositionedAuto(layout) => {
                    self.pending.push(Task::Yield(layout));
                    self.pending.push(Task::MiddleChildren(
                        layout.children().iter(),
                        MiddlePhase::Box,
                    ));
                    self.pending.push(Task::MiddleChildren(
                        layout.children().iter(),
                        MiddlePhase::Content,
                    ));
                }
                Task::FlexOrGridItem(layout) => {
                    self.pending.push(Task::Yield(layout));
                    self.pending.push(Task::MiddleChildren(
                        layout.children().iter(),
                        MiddlePhase::Box,
                    ));
                    self.pending.push(Task::MiddleChildren(
                        layout.children().iter(),
                        MiddlePhase::Content,
                    ));
                }
                Task::Middle(layout, phase) => {
                    if layout.establishes_stacking_context() || layout.is_positioned_auto() {
                        continue;
                    }
                    if layout.is_flex_or_grid_item_auto() {
                        if matches!(phase, MiddlePhase::Content) {
                            self.pending.push(Task::FlexOrGridItem(layout));
                        }
                        continue;
                    }

                    if phase.matches(layout) {
                        self.pending.push(Task::Yield(layout));
                    }
                    self.pending
                        .push(Task::MiddleChildren(layout.children().iter(), phase));
                }
                Task::MiddleChildren(mut children, phase) => {
                    if let Some(child) = children.next_back() {
                        self.pending.push(Task::MiddleChildren(children, phase));
                        self.pending.push(Task::Middle(child, phase));
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

        // `pending` is LIFO. Push the context root and lower layers first so
        // positive contexts, zero-level entries, ordinary content, negative
        // contexts, and finally the root are visited in reverse paint order.
        self.pending.push(Task::Yield(context_root));
        self.pending.extend(
            contexts
                .iter()
                .filter(|layout| layout.stacking_index() < 0)
                .map(|layout| Task::Context(layout)),
        );
        self.pending.push(Task::MiddleChildren(
            context_root.children().iter(),
            MiddlePhase::Box,
        ));
        self.pending.push(Task::MiddleChildren(
            context_root.children().iter(),
            MiddlePhase::Content,
        ));
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

impl MiddlePhase {
    fn matches(self, layout: &Layout) -> bool {
        matches!(
            (self, &layout.kind),
            (Self::Box, super::super::LayoutKind::Box { .. })
                | (Self::Content, super::super::LayoutKind::Text { .. })
        )
    }
}
