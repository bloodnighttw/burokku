use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use arc_swap::ArcSwap;
use thiserror::Error;
use tokio::sync::watch;

use super::Dom;

/// One immutable, atomically published DOM revision.
///
/// This wraps the existing [`Dom`] directly; it does not introduce a second
/// element or node representation.
#[derive(Clone, Debug)]
pub struct DomSnapshot {
    revision: u64,
    dom: Dom,
}

impl DomSnapshot {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn dom(&self) -> &Dom {
        &self.dom
    }
}

struct SharedDomInner {
    committed: ArcSwap<DomSnapshot>,
    commits: watch::Sender<u64>,
}

/// The thread-safe pointer shared by BTS and MTS.
///
/// MTS loads an `Arc<DomSnapshot>` once and may retain that exact value for a
/// complete frame. Notifications coalesce naturally through the watch channel.
#[derive(Clone)]
pub struct SharedDom {
    inner: Arc<SharedDomInner>,
}

impl SharedDom {
    pub fn new() -> Self {
        let initial = Arc::new(DomSnapshot {
            revision: 0,
            dom: Dom::new(),
        });
        let (commits, _) = watch::channel(0);
        Self {
            inner: Arc::new(SharedDomInner {
                committed: ArcSwap::new(initial),
                commits,
            }),
        }
    }

    /// Atomically load the latest complete DOM revision.
    pub fn load(&self) -> Arc<DomSnapshot> {
        self.inner.committed.load_full()
    }

    /// Subscribe to coalescing redraw/commit notifications.
    #[allow(dead_code)] // Consumed by tests now and the MTS frame loop in Phase 3.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.commits.subscribe()
    }

    fn publish(&self, snapshot: Arc<DomSnapshot>) {
        let revision = snapshot.revision;
        self.inner.committed.store(snapshot);
        self.inner.commits.send_replace(revision);
    }
}

impl Default for SharedDom {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SharedDom {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedDom")
            .field("committed_revision", &self.load().revision())
            .finish_non_exhaustive()
    }
}

/// BTS-owned mutable staging state and the pending batch flag.
#[derive(Debug)]
pub struct BtsDom {
    staging: Dom,
    shared: SharedDom,
    committed_revision: u64,
    dirty: bool,
    batch_depth: usize,
}

impl BtsDom {
    pub fn new(shared: SharedDom) -> Self {
        let initial = shared.load();
        assert_eq!(
            initial.revision(),
            0,
            "a BTS DOM owner must be created with a fresh SharedDom"
        );
        Self {
            staging: initial.dom().clone(),
            shared,
            committed_revision: 0,
            dirty: false,
            batch_depth: 0,
        }
    }

    /// Read staging, including successful changes not yet published.
    pub fn staging(&self) -> &Dom {
        &self.staging
    }

    /// Mutate staging and mark the pending batch only when the DOM revision
    /// actually changes. Invalid and no-op operations therefore remain clean.
    pub fn mutate(&mut self) -> StagingDomMut<'_> {
        let initial_revision = self.staging.revision();
        StagingDomMut {
            dom: &mut self.staging,
            dirty: &mut self.dirty,
            initial_revision,
        }
    }

    #[allow(dead_code)] // Useful to diagnostics and publication tests.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[allow(dead_code)] // Reserved for a future explicit JavaScript batch API.
    pub fn begin_batch(&mut self) -> Result<(), BatchError> {
        self.batch_depth = self
            .batch_depth
            .checked_add(1)
            .ok_or(BatchError::DepthOverflow)?;
        Ok(())
    }

    #[allow(dead_code)] // Reserved for a future explicit JavaScript batch API.
    pub fn end_batch(&mut self) -> Result<(), BatchError> {
        if self.batch_depth == 0 {
            return Err(BatchError::NotInBatch);
        }
        self.batch_depth -= 1;
        Ok(())
    }

    /// Publish one complete shallow clone of the existing DOM at the runtime
    /// checkpoint. The `SlotMap` entries are copied, but their `Arc<Node>`
    /// values share unchanged node contents with staging. Later staging writes
    /// copy only the affected nodes through `Arc::make_mut`.
    ///
    /// Clean checkpoints and checkpoints inside an explicit batch do nothing.
    pub fn checkpoint(&mut self) -> Result<Option<Arc<DomSnapshot>>, CommitError> {
        if !self.dirty || self.batch_depth != 0 {
            return Ok(None);
        }

        let revision = self
            .committed_revision
            .checked_add(1)
            .ok_or(CommitError::RevisionOverflow)?;
        let snapshot = Arc::new(DomSnapshot {
            revision,
            dom: self.staging.clone(),
        });
        self.shared.publish(snapshot.clone());
        self.committed_revision = revision;
        self.dirty = false;
        Ok(Some(snapshot))
    }
}

pub struct StagingDomMut<'a> {
    dom: &'a mut Dom,
    dirty: &'a mut bool,
    initial_revision: u64,
}

impl Deref for StagingDomMut<'_> {
    type Target = Dom;

    fn deref(&self) -> &Self::Target {
        self.dom
    }
}

impl DerefMut for StagingDomMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dom
    }
}

impl Drop for StagingDomMut<'_> {
    fn drop(&mut self) {
        if self.dom.revision() != self.initial_revision {
            *self.dirty = true;
        }
    }
}

#[allow(dead_code)] // Reserved for a future explicit JavaScript batch API.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum BatchError {
    #[error("end_batch called without a matching begin_batch")]
    NotInBatch,
    #[error("DOM batch nesting depth overflowed")]
    DepthOverflow,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum CommitError {
    // TODO: we will try to make overflow don't throw error in future.
    #[error("committed DOM revision counter overflowed")]
    RevisionOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::{Elements, NodeId};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn dom_and_snapshots_are_send_and_sync() {
        assert_send_sync::<Dom>();
        assert_send_sync::<DomSnapshot>();
        assert_send_sync::<SharedDom>();
    }

    #[test]
    fn readers_observe_old_or_new_complete_tree() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let old = shared.load();

        let (window, div) = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window);
            let div = dom.create(Elements::Div);
            dom.append_child(root, window).unwrap();
            dom.append_child(window, div).unwrap();
            (window, div)
        };

        assert_eq!(old.revision(), 0);
        assert!(!old.dom().contains(window));
        assert_eq!(shared.load().revision(), 0);

        owner.checkpoint().unwrap();
        let new = shared.load();

        assert_eq!(new.revision(), 1);
        assert_eq!(new.dom().parent(div), Some(window));
        assert!(!old.dom().contains(window));
    }

    #[test]
    fn staging_copies_only_a_node_changed_after_publication() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared);
        let (window, div) = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window);
            let div = dom.create(Elements::Div);
            dom.append_child(root, window).unwrap();
            dom.append_child(window, div).unwrap();
            (window, div)
        };
        let old = owner.checkpoint().unwrap().unwrap();

        assert!(owner.staging().shares_node_with(old.dom(), div));
        assert!(owner.staging().shares_node_with(old.dom(), window));
        owner
            .mutate()
            .set_attribute(div, "state".into(), "new".into())
            .unwrap();

        assert_eq!(old.dom().attribute(div, "state"), None);
        assert_eq!(owner.staging().attribute(div, "state"), Some("new"));
        assert!(!owner.staging().shares_node_with(old.dom(), div));
        assert!(owner.staging().shares_node_with(old.dom(), window));

        let new = owner.checkpoint().unwrap().unwrap();
        assert_eq!(old.dom().attribute(div, "state"), None);
        assert_eq!(new.dom().attribute(div, "state"), Some("new"));
        assert!(new.dom().shares_node_with(old.dom(), window));
    }

    #[test]
    fn structural_changes_preserve_each_snapshots_relationships() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared);
        let (first_parent, second_parent, child) = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window);
            let first_parent = dom.create(Elements::Div);
            let second_parent = dom.create(Elements::Div);
            let child = dom.create(Elements::Text);
            dom.append_child(root, window).unwrap();
            dom.append_child(window, first_parent).unwrap();
            dom.append_child(window, second_parent).unwrap();
            dom.append_child(first_parent, child).unwrap();
            (first_parent, second_parent, child)
        };
        let old = owner.checkpoint().unwrap().unwrap();

        owner.mutate().append_child(second_parent, child).unwrap();
        let new = owner.checkpoint().unwrap().unwrap();

        assert_eq!(old.dom().parent(child), Some(first_parent));
        assert_eq!(old.dom().children(first_parent), Some(&[child][..]));
        assert_eq!(old.dom().children(second_parent), Some(&[][..]));
        assert_eq!(new.dom().parent(child), Some(second_parent));
        assert_eq!(new.dom().children(first_parent), Some(&[][..]));
        assert_eq!(new.dom().children(second_parent), Some(&[child][..]));
    }

    #[test]
    fn removing_a_shared_subtree_leaves_old_snapshot_valid() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared);
        let (parent, child) = {
            let mut dom = owner.mutate();
            let parent = dom.create(Elements::Div);
            let child = dom.create(Elements::Text);
            dom.append_child(parent, child).unwrap();
            (parent, child)
        };
        let old = owner.checkpoint().unwrap().unwrap();

        assert!(owner.staging().shares_node_with(old.dom(), parent));
        assert_eq!(
            owner.mutate().remove_subtree(parent).unwrap(),
            Elements::Div
        );

        assert!(!owner.staging().contains(parent));
        assert!(!owner.staging().contains(child));
        assert!(old.dom().contains(parent));
        assert!(old.dom().contains(child));
        assert_eq!(old.dom().children(parent), Some(&[child][..]));
        assert_eq!(old.dom().parent(child), Some(parent));
    }

    #[test]
    fn validated_no_ops_do_not_copy_shared_nodes() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared);
        let (window, div) = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window);
            let div = dom.create(Elements::Div);
            dom.set_attribute(div, "state".into(), "same".into())
                .unwrap();
            dom.append_child(root, window).unwrap();
            dom.append_child(window, div).unwrap();
            (window, div)
        };
        let snapshot = owner.checkpoint().unwrap().unwrap();

        {
            let mut dom = owner.mutate();
            dom.set_attribute(div, "state".into(), "same".into())
                .unwrap();
            dom.append_child(window, div).unwrap();
        }

        assert!(!owner.is_dirty());
        assert!(owner.staging().shares_node_with(snapshot.dom(), window));
        assert!(owner.staging().shares_node_with(snapshot.dom(), div));
    }

    #[test]
    fn several_mutations_publish_once() {
        let shared = SharedDom::new();
        let mut notifications = shared.subscribe();
        let mut owner = BtsDom::new(shared.clone());

        {
            let mut dom = owner.mutate();
            dom.create(Elements::Div);
            dom.create(Elements::Text);
            dom.create(Elements::_String {
                string: "content".into(),
            });
        }

        owner.checkpoint().unwrap();
        assert_eq!(*notifications.borrow_and_update(), 1);
        assert!(!notifications.has_changed().unwrap());
        assert!(owner.checkpoint().unwrap().is_none());
        assert!(!notifications.has_changed().unwrap());
    }

    #[test]
    fn invalid_and_no_op_mutations_do_not_dirty_the_batch() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared);
        let missing = NodeId::default();

        {
            let mut dom = owner.mutate();
            assert!(dom.set_element(missing, Elements::Div).is_err());
            let root = dom.root();
            dom.set_element(root, Elements::App).unwrap();
        }

        assert!(!owner.is_dirty());
        assert!(owner.checkpoint().unwrap().is_none());
    }

    #[test]
    fn nested_batches_defer_publication() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        owner.begin_batch().unwrap();
        owner.begin_batch().unwrap();
        owner.mutate().create(Elements::Div);

        assert!(owner.checkpoint().unwrap().is_none());
        owner.end_batch().unwrap();
        assert!(owner.checkpoint().unwrap().is_none());
        owner.end_batch().unwrap();
        assert!(owner.checkpoint().unwrap().is_some());
        assert_eq!(shared.load().revision(), 1);
    }

    #[test]
    fn stale_handles_remain_stale_after_publication() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let stale = {
            let mut dom = owner.mutate();
            let stale = dom.create(Elements::Div);
            dom.remove_subtree(stale).unwrap();
            stale
        };

        owner.checkpoint().unwrap();
        assert!(!shared.load().dom().contains(stale));
        assert!(!owner.staging().contains(stale));
    }
}
