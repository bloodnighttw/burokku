#![allow(
    dead_code,
    reason = "publication types are wired into the DOM owner in subsequent implementation steps"
)]

use std::{fmt, sync::Arc};

use arc_swap::ArcSwap;

use super::{Dom, DomIter, Element, Node, NodeId, NodeKind};

/// Describes how an MTS consumer should reconcile two committed DOM revisions.
///
/// The initial publication implementation always rebuilds all computed state.
/// Bounded incremental change batches can be added as another variant later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeSet {
    FullRebuild {
        from_revision: u64,
        to_revision: u64,
    },
}

impl ChangeSet {
    pub fn source_revision(self) -> u64 {
        match self {
            Self::FullRebuild { from_revision, .. } => from_revision,
        }
    }

    pub fn target_revision(self) -> u64 {
        match self {
            Self::FullRebuild { to_revision, .. } => to_revision,
        }
    }
}

/// An immutable committed view of one complete DOM revision.
///
/// The contained [`Dom`] remains private so consumers can only receive
/// read-only accessors. BTS creates a snapshot by cloning its staging arena;
/// subsequent staging mutations use the arena's existing copy-on-write nodes.
#[derive(Debug)]
pub struct DomSnapshot {
    dom: Dom,
}

impl DomSnapshot {
    pub(super) fn from_staging(dom: &Dom) -> Self {
        // clone the staging arena to create an immutable snapshot
        // it won't affect the staging arena the BTS owns
        Self { dom: dom.clone() }
    }

    pub fn revision(&self) -> u64 {
        self.dom.revision()
    }

    pub fn root(&self) -> NodeId {
        self.dom.root()
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.dom.contains(id)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.dom.node(id)
    }

    pub fn kind(&self, id: NodeId) -> Option<&NodeKind> {
        self.dom.kind(id)
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        self.dom.element(id)
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        self.dom.text(id)
    }

    pub fn attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.dom.attribute(id, name)
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.dom.parent(id)
    }

    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        self.dom.children(id)
    }

    /// Iterate over the app tree in pre-order. Detached nodes are not visited.
    pub fn iter(&self) -> DomIter<'_> {
        self.dom.iter()
    }
}

impl<'a> IntoIterator for &'a DomSnapshot {
    type Item = (NodeId, &'a NodeKind);
    type IntoIter = DomIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A snapshot and its reconciliation marker, published as one atomic value.
#[derive(Debug)]
pub struct PublishedDom {
    snapshot: DomSnapshot,
    changes: ChangeSet,
}

impl PublishedDom {
    pub(super) fn new(snapshot: DomSnapshot, changes: ChangeSet) -> Self {
        assert_eq!(
            snapshot.revision(),
            changes.target_revision(),
            "a change set must target its enclosed DOM snapshot"
        );
        Self { snapshot, changes }
    }

    pub fn snapshot(&self) -> &DomSnapshot {
        &self.snapshot
    }

    pub fn changes(&self) -> ChangeSet {
        self.changes
    }

    pub fn revision(&self) -> u64 {
        self.snapshot.revision()
    }
}

/// Notifies MTS that a committed DOM revision is available.
///
/// Implementations should only wake or signal the main event loop. They must
/// not run layout, rendering, or staging DOM work on the publisher's thread (BTS).
pub(crate) trait CommitNotifier: Send + 'static {
    fn committed(&self, revision: u64);
}

impl<F> CommitNotifier for F
where
    F: Fn(u64) + Send + 'static,
{
    fn committed(&self, revision: u64) {
        self(revision);
    }
}

/// The cloneable MTS side of immutable DOM publication.
#[derive(Clone)]
pub struct PublishedDomReader {
    committed: Arc<ArcSwap<PublishedDom>>,
}

impl PublishedDomReader {
    /// Load and retain one complete publication.
    ///
    /// A frame should call this once and keep the returned `Arc` through
    /// reconciliation, layout, scene construction, and presentation.
    pub fn load(&self) -> Arc<PublishedDom> {
        self.committed.load_full()
    }
}

impl fmt::Debug for PublishedDomReader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PublishedDomReader")
            .field("revision", &self.committed.load().revision())
            .finish_non_exhaustive()
    }
}

/// The single-owner BTS side of immutable DOM publication.
///
/// Publication behavior is added in the checkpoint implementation step. The
/// writer is `Send` so it can move to BTS, but its single-owner notifier keeps
/// it from being `Sync`; it also intentionally does not implement [`Clone`].
pub(crate) struct DomPublisher {
    committed: Arc<ArcSwap<PublishedDom>>,
    last_published_revision: u64,
    notifier: Box<dyn CommitNotifier>,
}

impl fmt::Debug for DomPublisher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DomPublisher")
            .field("last_published_revision", &self.last_published_revision)
            .field("committed_revision", &self.committed.load().revision())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicU64, atomic::Ordering, Arc};

    use super::*;

    fn baseline(dom: &Dom) -> Arc<PublishedDom> {
        Arc::new(PublishedDom::new(
            DomSnapshot::from_staging(dom),
            ChangeSet::FullRebuild {
                from_revision: dom.revision(),
                to_revision: dom.revision(),
            },
        ))
    }

    #[test]
    fn publication_keeps_snapshot_and_change_revisions_together() {
        let dom = Dom::new();
        let published = baseline(&dom);

        assert_eq!(published.revision(), 0);
        assert_eq!(published.snapshot().revision(), 0);
        assert_eq!(published.changes().source_revision(), 0);
        assert_eq!(published.changes().target_revision(), 0);
    }

    #[test]
    #[should_panic(expected = "a change set must target its enclosed DOM snapshot")]
    fn rejects_a_change_marker_for_another_snapshot_revision() {
        let dom = Dom::new();

        PublishedDom::new(
            DomSnapshot::from_staging(&dom),
            ChangeSet::FullRebuild {
                from_revision: 0,
                to_revision: 1,
            },
        );
    }

    #[test]
    fn snapshot_exposes_read_only_queries_and_preserves_node_ids() {
        let mut staging = Dom::new();
        let window = staging.create_element(Element::Window {
            style: Box::default(),
        });
        let div = staging.create_element(Element::Div {
            style: Box::default(),
        });
        let text = staging.create_text("before");
        let detached = staging.create_element(Element::Grid {
            style: Box::default(),
        });
        staging
            .set_attribute(div, "role".into(), "status".into())
            .unwrap();
        staging.append_child(staging.root(), window).unwrap();
        staging.append_child(window, div).unwrap();
        staging.append_child(div, text).unwrap();

        let snapshot = DomSnapshot::from_staging(&staging);
        let snapshot_revision = snapshot.revision();

        assert_eq!(snapshot.root(), staging.root());
        assert!(snapshot.contains(detached));
        assert!(matches!(
            snapshot.kind(snapshot.root()),
            Some(NodeKind::App)
        ));
        assert!(matches!(
            snapshot.element(window),
            Some(Element::Window { .. })
        ));
        assert_eq!(snapshot.text(text), Some("before"));
        assert_eq!(snapshot.attribute(div, "role"), Some("status"));
        assert_eq!(snapshot.parent(text), Some(div));
        assert_eq!(snapshot.children(div), Some(&[text][..]));
        assert_eq!(snapshot.node(text).unwrap().parent(), Some(div));
        assert_eq!(
            snapshot.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![snapshot.root(), window, div, text]
        );
        assert_eq!((&snapshot).into_iter().count(), 4);

        staging.set_text(text, "after").unwrap();
        staging.remove_subtree(div).unwrap();

        assert_eq!(snapshot.revision(), snapshot_revision);
        assert_eq!(snapshot.text(text), Some("before"));
        assert_eq!(snapshot.parent(text), Some(div));
        assert!(snapshot.contains(div));
        assert!(snapshot.contains(text));
        assert!(!staging.contains(div));
        assert!(!staging.contains(text));
    }

    #[test]
    fn reader_loads_an_owned_complete_publication() {
        let dom = Dom::new();
        let committed = Arc::new(ArcSwap::from(baseline(&dom)));
        let reader = PublishedDomReader {
            committed: committed.clone(),
        };

        let frame_revision = reader.load();
        assert_eq!(frame_revision.revision(), 0);
        assert_eq!(Arc::strong_count(&frame_revision), 2);
        assert!(format!("{reader:?}").contains("revision: 0"));
    }

    #[test]
    fn closures_can_notify_committed_revisions() {
        let notified_revision = Arc::new(AtomicU64::new(0));
        let notifier: Box<dyn CommitNotifier> = Box::new({
            let notified_revision = notified_revision.clone();
            move |revision| notified_revision.store(revision, Ordering::Release)
        });

        notifier.committed(42);

        assert_eq!(notified_revision.load(Ordering::Acquire), 42);
    }

    #[test]
    fn publication_handles_have_the_intended_thread_traits() {
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<DomSnapshot>();
        assert_send_sync::<PublishedDom>();
        assert_send_sync::<PublishedDomReader>();
        assert_send::<DomPublisher>();
    }
}
