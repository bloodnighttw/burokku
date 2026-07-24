use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

use super::{Document, DocumentError, ElementKind};

#[derive(Clone, Debug)]
pub struct UiStore {
    inner: Arc<UiStoreInner>,
}

#[derive(Debug)]
struct UiStoreInner {
    // The document and its version are published together so readers cannot
    // observe a version from one update and a document from another.
    state: ArcSwap<UiState>,
    // Readers never acquire this lock; it only prevents concurrent writers
    // from replacing each other's updates.
    writer: Mutex<()>,
}

#[derive(Debug)]
struct UiState {
    document: Arc<Document>,
    version: u64,
}

impl Default for UiStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UiStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(UiStoreInner {
                state: ArcSwap::from_pointee(UiState {
                    document: Arc::new(Document::new()),
                    version: 0,
                }),
                writer: Mutex::new(()),
            }),
        }
    }

    pub fn snapshot(&self) -> Arc<Document> {
        Arc::clone(&self.inner.state.load().document)
    }

    pub fn snapshot_with_version(&self) -> (u64, Arc<Document>) {
        let state = self.inner.state.load();
        (state.version, Arc::clone(&state.document))
    }

    pub fn snapshot_if_changed(&self, version: u64) -> Option<(u64, Arc<Document>)> {
        let state = self.inner.state.load();
        (state.version != version).then(|| (state.version, Arc::clone(&state.document)))
    }

    pub fn create_node(&self, kind: ElementKind) -> u64 {
        let _writer = self.writer();
        let state = self.inner.state.load_full();
        let mut document = (*state.document).clone();
        let id = document.create_node(kind);
        self.publish(&state, document);
        id
    }

    pub fn set_text(&self, id: u64, text: String) -> Result<(), DocumentError> {
        self.mutate(|document| document.set_text(id, text))
    }

    pub fn set_style(&self, id: u64, name: &str, value: Option<&str>) -> Result<(), DocumentError> {
        self.mutate(|document| document.set_style(id, name, value))
    }

    pub fn set_attribute(
        &self,
        id: u64,
        name: &str,
        value: Option<&str>,
    ) -> Result<(), DocumentError> {
        self.mutate(|document| document.set_attribute(id, name, value))
    }

    pub fn insert(
        &self,
        parent: u64,
        child: u64,
        before: Option<u64>,
    ) -> Result<(), DocumentError> {
        self.mutate(|document| document.insert(parent, child, before))
    }

    pub fn remove(&self, parent: u64, child: u64) -> Result<(), DocumentError> {
        self.mutate(|document| document.remove(parent, child))
    }

    fn writer(&self) -> std::sync::MutexGuard<'_, ()> {
        self.inner
            .writer
            .lock()
            .expect("the UI document writer lock is not poisoned")
    }

    fn mutate(
        &self,
        mutation: impl FnOnce(&mut Document) -> Result<(), DocumentError>,
    ) -> Result<(), DocumentError> {
        let _writer = self.writer();
        let state = self.inner.state.load_full();
        let mut document = (*state.document).clone();
        mutation(&mut document)?;
        self.publish(&state, document);
        Ok(())
    }

    fn publish(&self, previous: &UiState, document: Document) {
        self.inner.state.store(Arc::new(UiState {
            document: Arc::new(document),
            version: previous.version.wrapping_add(1),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_pair_the_document_with_its_version() {
        let store = UiStore::new();
        assert!(store.snapshot_if_changed(0).is_none());

        let first = store.create_node(ElementKind::Text(String::new()));
        let (version, snapshot) = store
            .snapshot_if_changed(0)
            .expect("creating a node changes the UI");
        assert_eq!(version, 1);
        assert_eq!(
            snapshot.node(first).unwrap().kind,
            ElementKind::Text(String::new())
        );
        assert!(store.snapshot_if_changed(version).is_none());
    }

    #[test]
    fn failed_mutations_do_not_advance_the_version() {
        let store = UiStore::new();
        let (version, _) = store.snapshot_with_version();

        assert!(store.set_text(999, "missing".into()).is_err());
        assert_eq!(store.snapshot_with_version().0, version);
    }

    #[test]
    fn snapshots_remain_stable_while_the_document_changes() {
        let store = UiStore::new();
        let text = store.create_node(ElementKind::Text(String::new()));
        store.set_text(text, "before".into()).unwrap();
        let before = store.snapshot();

        store.set_text(text, "after".into()).unwrap();
        let after = store.snapshot();

        assert_eq!(
            before.node(text).unwrap().kind,
            ElementKind::Text("before".into())
        );
        assert_eq!(
            after.node(text).unwrap().kind,
            ElementKind::Text("after".into())
        );
    }

    #[test]
    fn snapshots_do_not_wait_for_the_writer_lock() {
        let store = UiStore::new();
        let _writer = store.writer();
        let reader = store.clone();
        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::channel();

        let reader_thread = std::thread::spawn(move || {
            snapshot_tx.send(reader.snapshot_with_version()).unwrap();
        });

        snapshot_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("snapshot reads must not acquire the writer lock");
        reader_thread.join().unwrap();
    }
}
