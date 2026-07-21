use std::sync::{Arc, Mutex};

use super::{Document, DomError, NodeKind};

#[derive(Clone, Debug)]
pub struct DomStore {
    inner: Arc<DomStoreInner>,
}

#[derive(Debug)]
struct DomStoreInner {
    state: Mutex<DomState>,
}

#[derive(Debug)]
struct DomState {
    document: Arc<Document>,
    version: u64,
}

impl Default for DomStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DomStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DomStoreInner {
                state: Mutex::new(DomState {
                    document: Arc::new(Document::new()),
                    version: 0,
                }),
            }),
        }
    }

    pub fn snapshot(&self) -> Arc<Document> {
        Arc::clone(&self.state().document)
    }

    pub fn snapshot_with_version(&self) -> (u64, Arc<Document>) {
        let state = self.state();
        (state.version, Arc::clone(&state.document))
    }

    pub fn snapshot_if_changed(&self, version: u64) -> Option<(u64, Arc<Document>)> {
        let state = self.state();
        (state.version != version).then(|| (state.version, Arc::clone(&state.document)))
    }

    pub fn create_node(&self, kind: NodeKind) -> u64 {
        let mut state = self.state();
        let id = Arc::make_mut(&mut state.document).create_node(kind);
        state.changed();
        id
    }

    pub fn set_text(&self, id: u64, text: String) -> Result<(), DomError> {
        self.mutate(|document| document.set_text(id, text))
    }

    pub fn set_style(&self, id: u64, name: &str, value: Option<&str>) -> Result<(), DomError> {
        self.mutate(|document| document.set_style(id, name, value))
    }

    pub fn insert(&self, parent: u64, child: u64, before: Option<u64>) -> Result<(), DomError> {
        self.mutate(|document| document.insert(parent, child, before))
    }

    pub fn remove(&self, parent: u64, child: u64) -> Result<(), DomError> {
        self.mutate(|document| document.remove(parent, child))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, DomState> {
        self.inner
            .state
            .lock()
            .expect("the DOM document lock is not poisoned")
    }

    fn mutate(
        &self,
        mutation: impl FnOnce(&mut Document) -> Result<(), DomError>,
    ) -> Result<(), DomError> {
        let mut state = self.state();
        mutation(Arc::make_mut(&mut state.document))?;
        state.changed();
        Ok(())
    }
}

impl DomState {
    fn changed(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_pair_the_document_with_its_version() {
        let store = DomStore::new();
        assert!(store.snapshot_if_changed(0).is_none());

        let first = store.create_node(NodeKind::Text);
        let (version, snapshot) = store
            .snapshot_if_changed(0)
            .expect("creating a node changes the DOM");
        assert_eq!(version, 1);
        assert_eq!(snapshot.node(first).unwrap().kind, NodeKind::Text);
        assert!(store.snapshot_if_changed(version).is_none());
    }

    #[test]
    fn failed_mutations_do_not_advance_the_version() {
        let store = DomStore::new();
        let (version, _) = store.snapshot_with_version();

        assert!(store.set_text(999, "missing".into()).is_err());
        assert_eq!(store.snapshot_with_version().0, version);
    }

    #[test]
    fn snapshots_remain_stable_while_the_document_changes() {
        let store = DomStore::new();
        let text = store.create_node(NodeKind::Text);
        store.set_text(text, "before".into()).unwrap();
        let before = store.snapshot();

        store.set_text(text, "after".into()).unwrap();
        let after = store.snapshot();

        assert_eq!(before.node(text).unwrap().text, "before");
        assert_eq!(after.node(text).unwrap().text, "after");
    }
}
