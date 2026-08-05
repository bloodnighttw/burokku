use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

#[derive(Clone)]
pub struct UiStore {
    inner: Arc<UiStoreInner>,
}

struct UiStoreInner {
    state: ArcSwap<UiState>,
    writer: Mutex<()>,
}

struct UiState {
    serialized: Arc<String>,
    version: u64,
}

impl UiStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(UiStoreInner {
                state: ArcSwap::from_pointee(UiState {
                    serialized: Arc::new(r#"{"type":"app","children":[]}"#.into()),
                    version: 0,
                }),
                writer: Mutex::new(()),
            }),
        }
    }

    pub fn snapshot(&self) -> Arc<String> {
        Arc::clone(&self.inner.state.load().serialized)
    }

    pub fn snapshot_with_version(&self) -> (u64, Arc<String>) {
        let state = self.inner.state.load();
        (state.version, Arc::clone(&state.serialized))
    }

    pub fn snapshot_if_changed(&self, version: u64) -> Option<(u64, Arc<String>)> {
        let state = self.inner.state.load();
        (state.version != version).then(|| (state.version, Arc::clone(&state.serialized)))
    }

    pub fn version(&self) -> u64 {
        self.inner.state.load().version
    }

    pub fn replace(&self, serialized: String) {
        let _writer = self
            .inner
            .writer
            .lock()
            .expect("the UI tree writer lock is not poisoned");
        let previous = self.inner.state.load_full();
        self.inner.state.store(Arc::new(UiState {
            serialized: Arc::new(serialized),
            version: previous.version.wrapping_add(1),
        }));
    }
}

impl Default for UiStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_the_tree_publishes_a_new_versioned_snapshot() {
        let store = UiStore::new();
        let before = store.snapshot();

        store.replace(r#"{"type":"app","children":[{"type":"window"}]}"#.into());

        let (version, after) = store.snapshot_with_version();
        assert_eq!(version, 1);
        assert!(!Arc::ptr_eq(&before, &after));
        assert!(store.snapshot_if_changed(version).is_none());
    }
}
