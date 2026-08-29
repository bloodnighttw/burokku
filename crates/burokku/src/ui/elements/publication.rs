//! Atomic transfer of immutable DOM revisions from BTS to MTS.
//!
//! # Producer integration
//!
//! The DOM owner holds its mutable staging [`Dom`] and one [`DomPublisher`].
//! Mutations are published only after their scheduling batch completes, so a
//! consumer never observes a partially processed JavaScript task. The staging
//! DOM, publisher, and notifier remain crate-private capabilities.
//! # MTS frame integration
//!
//! MTS owns a cloneable [`PublishedDomReader`]. It calls
//! [`PublishedDomReader::load`] once at frame start and retains that exact
//! `Arc<PublishedDom>` through reconciliation, layout, hit testing, scene
//! construction, and presentation. Loading individual pieces again during the
//! frame could mix revisions and is therefore outside this API's contract.
//!
//! The notifier only wakes MTS after the new publication is stored. Native
//! window ownership, redraw handling, Taffy reconciliation, Vello rendering,
//! events, and the JavaScript node facade are separate implementation stages.

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
/// The DOM owner keeps this writer alongside its mutable staging [`Dom`] and
/// publishes only at an explicit scheduler boundary.
/// The writer is `Send` so it can move to BTS, but its single-owner notifier
/// keeps it from being `Sync`; it also intentionally does not implement
/// [`Clone`].
pub(crate) struct DomPublisher {
    committed: Arc<ArcSwap<PublishedDom>>,
    last_published_revision: u64,
    notifier: Box<dyn CommitNotifier>,
}

impl DomPublisher {
    /// Create the single BTS writer and cloneable MTS reader.
    ///
    /// The initial staging state becomes the immutable baseline without
    /// emitting a notification.
    ///
    /// you should pass your custom notifier, for example:
    /// ```ignore
    /// let (publisher, reader) = DomPublisher::new(&staging, {
    ///     let count = Arc::new(AtomicUsize::new(0));
    ///     let count_clone = count.clone();
    ///     move |_| {
    ///         count_clone.fetch_add(1, Ordering::AcqRel);
    ///     }
    /// });
    /// ```
    pub(crate) fn new(staging: &Dom, notifier: impl CommitNotifier) -> (Self, PublishedDomReader) {
        let revision = staging.revision();
        let baseline = Arc::new(PublishedDom::new(
            DomSnapshot::from_staging(staging),
            ChangeSet::FullRebuild {
                from_revision: revision,
                to_revision: revision,
            },
        ));
        let committed = Arc::new(ArcSwap::from(baseline));
        let reader = PublishedDomReader {
            committed: committed.clone(),
        };
        let publisher = Self {
            committed,
            last_published_revision: revision,
            notifier: Box::new(notifier),
        };

        (publisher, reader)
    }

    /// Publish one complete revision when staging changed since the previous
    /// checkpoint.
    ///
    /// The atomic store happens before MTS is notified, so a consumer awakened
    /// by the notifier can immediately load the committed target revision.
    pub(crate) fn checkpoint(&mut self, staging: &Dom) -> Option<u64> {
        let target_revision = staging.revision();
        if target_revision == self.last_published_revision {
            return None;
        }

        let source_revision = self.last_published_revision;
        let publication = Arc::new(PublishedDom::new(
            DomSnapshot::from_staging(staging),
            ChangeSet::FullRebuild {
                from_revision: source_revision,
                to_revision: target_revision,
            },
        ));

        self.committed.store(publication);
        self.last_published_revision = target_revision;
        self.notifier.committed(target_revision);
        Some(target_revision)
    }
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
    use std::sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    };

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
    fn publisher_initializes_without_notifying_and_skips_unchanged_checkpoints() {
        let staging = Dom::new();
        let notification_count = Arc::new(AtomicUsize::new(0));
        let (mut publisher, reader) = DomPublisher::new(&staging, {
            let notification_count = notification_count.clone();
            move |_| {
                notification_count.fetch_add(1, Ordering::AcqRel);
            }
        });

        let before = reader.load();
        assert_eq!(before.revision(), 0);
        assert!(matches!(
            before.snapshot().kind(before.snapshot().root()),
            Some(NodeKind::App)
        ));
        assert_eq!(publisher.checkpoint(&staging), None);

        let after = reader.load();
        assert!(Arc::ptr_eq(&before, &after));
        assert_eq!(notification_count.load(Ordering::Acquire), 0);
    }

    #[test]
    fn checkpoint_coalesces_mutations_and_retains_complete_old_revisions() {
        let mut staging = Dom::new();
        let notified_revisions = Arc::new(Mutex::new(Vec::new()));
        let (mut publisher, reader) = DomPublisher::new(&staging, {
            let notified_revisions = notified_revisions.clone();
            move |revision| notified_revisions.lock().unwrap().push(revision)
        });
        let old_frame = reader.load();

        let window = staging.create_element(Element::Window {
            style: Box::default(),
        });
        let div = staging.create_element(Element::Div {
            style: Box::default(),
        });
        let paragraph = staging.create_element(Element::Text {
            style: Box::default(),
        });
        let text = staging.create_text("before");
        staging.append_child(staging.root(), window).unwrap();
        staging.append_child(window, div).unwrap();
        staging.append_child(div, paragraph).unwrap();
        staging.append_child(paragraph, text).unwrap();
        staging
            .set_attribute(div, "role".into(), "status".into())
            .unwrap();
        let first_revision = staging.revision();

        assert_eq!(publisher.checkpoint(&staging), Some(first_revision));
        let first_frame = reader.load();

        assert_eq!(old_frame.revision(), 0);
        assert_eq!(old_frame.snapshot().iter().count(), 1);
        assert!(!old_frame.snapshot().contains(window));
        assert_eq!(first_frame.revision(), first_revision);
        assert_eq!(first_frame.snapshot().parent(text), Some(paragraph));
        assert_eq!(first_frame.snapshot().text(text), Some("before"));
        assert_eq!(
            first_frame.snapshot().attribute(div, "role"),
            Some("status")
        );
        assert_eq!(first_frame.changes().source_revision(), 0);
        assert_eq!(first_frame.changes().target_revision(), first_revision);
        assert_eq!(*notified_revisions.lock().unwrap(), [first_revision]);

        staging.set_text(text, "after").unwrap();
        assert_eq!(reader.load().snapshot().text(text), Some("before"));
        assert_eq!(old_frame.snapshot().iter().count(), 1);

        let second_revision = staging.revision();
        assert_eq!(publisher.checkpoint(&staging), Some(second_revision));
        let second_frame = reader.load();
        assert_eq!(second_frame.snapshot().text(text), Some("after"));
        assert_eq!(first_frame.snapshot().text(text), Some("before"));
        assert_eq!(second_frame.changes().source_revision(), first_revision);
        assert_eq!(second_frame.changes().target_revision(), second_revision);
        assert_eq!(
            *notified_revisions.lock().unwrap(),
            [first_revision, second_revision]
        );
    }

    #[test]
    fn no_op_and_invalid_mutations_do_not_publish() {
        let mut staging = Dom::new();
        let paragraph = staging.create_element(Element::Text {
            style: Box::default(),
        });
        let text = staging.create_text("same");
        staging
            .set_attribute(paragraph, "role".into(), "status".into())
            .unwrap();
        staging.append_child(paragraph, text).unwrap();
        let baseline_revision = staging.revision();
        let notification_count = Arc::new(AtomicUsize::new(0));
        let (mut publisher, reader) = DomPublisher::new(&staging, {
            let notification_count = notification_count.clone();
            move |_| {
                notification_count.fetch_add(1, Ordering::AcqRel);
            }
        });
        let baseline = reader.load();

        staging.set_text(text, "same").unwrap();
        staging
            .set_attribute(paragraph, "role".into(), "status".into())
            .unwrap();
        staging.append_child(paragraph, text).unwrap();
        staging.detach(paragraph).unwrap();
        assert!(matches!(
            staging.append_child(text, paragraph),
            Err(super::super::DomError::InvalidRelationship { .. })
        ));

        assert_eq!(staging.revision(), baseline_revision);
        assert_eq!(publisher.checkpoint(&staging), None);
        assert!(Arc::ptr_eq(&baseline, &reader.load()));
        assert_eq!(notification_count.load(Ordering::Acquire), 0);
    }

    #[test]
    fn checkpoint_stores_before_notifying() {
        let mut staging = Dom::new();
        let reader_slot = Arc::new(Mutex::new(None::<PublishedDomReader>));
        let observations = Arc::new(Mutex::new(Vec::new()));
        let (mut publisher, reader) = DomPublisher::new(&staging, {
            let reader_slot = reader_slot.clone();
            let observations = observations.clone();
            move |notified_revision| {
                let loaded_revision = reader_slot
                    .lock()
                    .unwrap()
                    .as_ref()
                    .expect("reader is installed before the first checkpoint")
                    .load()
                    .revision();
                observations
                    .lock()
                    .unwrap()
                    .push((notified_revision, loaded_revision));
            }
        });
        *reader_slot.lock().unwrap() = Some(reader);

        staging.create_text("detached mutation");
        let revision = staging.revision();
        assert_eq!(publisher.checkpoint(&staging), Some(revision));

        assert_eq!(*observations.lock().unwrap(), [(revision, revision)]);
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
        let paragraph = staging.create_element(Element::Text {
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
        staging.append_child(div, paragraph).unwrap();
        staging.append_child(paragraph, text).unwrap();

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
        assert_eq!(snapshot.parent(text), Some(paragraph));
        assert_eq!(snapshot.children(div), Some(&[paragraph][..]));
        assert_eq!(snapshot.children(paragraph), Some(&[text][..]));
        assert_eq!(snapshot.node(text).unwrap().parent(), Some(paragraph));
        assert_eq!(
            snapshot.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![snapshot.root(), window, div, paragraph, text]
        );
        assert_eq!((&snapshot).into_iter().count(), 5);

        staging.set_text(text, "after").unwrap();
        staging.remove_subtree(div).unwrap();

        assert_eq!(snapshot.revision(), snapshot_revision);
        assert_eq!(snapshot.text(text), Some("before"));
        assert_eq!(snapshot.parent(text), Some(paragraph));
        assert!(snapshot.contains(div));
        assert!(snapshot.contains(paragraph));
        assert!(snapshot.contains(text));
        assert!(!staging.contains(div));
        assert!(!staging.contains(paragraph));
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
