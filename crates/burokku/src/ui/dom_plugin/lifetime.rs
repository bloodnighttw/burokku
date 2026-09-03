//! DOM node lifetime ownership rules.
//!
//! 1. The app tree retains every connected descendant, even without JavaScript wrappers.
//! 2. A live canonical JavaScript wrapper retains its entire detached component so parent
//!    and sibling traversal remains valid.
//! 3. `NodeId` values held by layout, rendering, or other derived state are non-owning.
//! 4. Dropping the final wrapper only makes a detached component eligible for reclamation;
//!    cleanup happens after QuickJS GC at the next host maintenance point (in about_to_wait).
//! 5. Every use of a retained `NodeId` must validate its generation because reclamation can
//!    make non-owning handles stale.

use std::{cell::RefCell, collections::HashSet, rc::Rc};

use slotmap::Key;

use super::UiDomState;
use crate::ui::elements::{DomError, NodeId};

pub(super) type SharedWrapperRoots = Rc<RefCell<WrapperRoots>>;

#[derive(Debug, Default)]
pub(super) struct WrapperRoots {
    nodes: HashSet<NodeId>,
}

impl WrapperRoots {
    fn acquire(&mut self, id: NodeId) {
        assert!(
            self.nodes.insert(id),
            "canonical NodeId wrappers cannot overlap"
        );
    }

    pub(super) fn release(&mut self, id: NodeId) {
        debug_assert!(self.nodes.remove(&id), "wrapper root must be registered");
    }
}

pub(super) fn encode_node_id(id: NodeId) -> String {
    format!("{:016x}", id.data().as_ffi())
}

impl UiDomState {
    pub(super) fn acquire_wrapper(&self, id: NodeId) -> Result<SharedWrapperRoots, DomError> {
        self.dom
            .contains(id)
            .then_some(())
            .ok_or(DomError::NodeNotFound(id))?;
        self.wrapper_roots.borrow_mut().acquire(id);
        Ok(Rc::clone(&self.wrapper_roots))
    }

    pub(crate) fn reclaim_detached(&mut self) -> runtime::Result<()> {
        let live = self
            .wrapper_roots
            .borrow()
            .nodes
            .iter()
            .copied()
            .collect::<Vec<_>>();
        self.last_reclaim = self
            .dom
            .reclaim_unreachable_detached(live)
            .map_err(|error| {
                runtime::Error::new_from_js_message(
                    "DOM wrapper roots",
                    "live NodeId values",
                    error.to_string(),
                )
            })?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn live_wrapper_count(&self) -> usize {
        self.wrapper_roots.borrow().nodes.len()
    }

    #[cfg(test)]
    pub(super) fn has_wrapper(&self, id: NodeId) -> bool {
        self.wrapper_roots.borrow().nodes.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_tokens_preserve_full_node_id_precision() {
        let mut dom = crate::ui::elements::Dom::new();
        let token = encode_node_id(dom.create_text("token"));

        assert_eq!(token.len(), 16);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
