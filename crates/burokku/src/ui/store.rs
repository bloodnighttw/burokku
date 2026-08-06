use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

use super::Elements;

#[derive(Clone)]
pub struct UiStore {
    inner: Arc<UiStoreInner>,
}

struct UiStoreInner {
    state: ArcSwap<UiState>,
    writer: Mutex<()>,
}

struct UiState {
    tree: Arc<Elements>,
    version: u64,
}

impl UiStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(UiStoreInner {
                state: ArcSwap::from_pointee(UiState {
                    tree: Arc::new(Elements::App {
                        children: Vec::new(),
                    }),
                    version: 0,
                }),
                writer: Mutex::new(()),
            }),
        }
    }

    pub fn snapshot(&self) -> Arc<Elements> {
        Arc::clone(&self.inner.state.load().tree)
    }

    pub fn snapshot_with_version(&self) -> (u64, Arc<Elements>) {
        let state = self.inner.state.load();
        (state.version, Arc::clone(&state.tree))
    }

    pub fn snapshot_if_changed(&self, version: u64) -> Option<(u64, Arc<Elements>)> {
        let state = self.inner.state.load();
        (state.version != version).then(|| (state.version, Arc::clone(&state.tree)))
    }

    pub fn replace(&self, tree: Elements) {
        let _writer = self
            .inner
            .writer
            .lock()
            .expect("the UI tree writer lock is not poisoned");
        let previous = self.inner.state.load_full();
        self.inner.state.store(Arc::new(UiState {
            tree: Arc::new(tree),
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

        store.replace(Elements::App {
            children: vec![Elements::Window {
                children: Vec::new(),
            }],
        });

        let (version, after) = store.snapshot_with_version();
        assert_eq!(version, 1);
        assert!(!Arc::ptr_eq(&before, &after));
        assert!(matches!(
            after.as_ref(),
            Elements::App { children } if matches!(children.as_slice(), [Elements::Window { .. }])
        ));
        assert!(store.snapshot_if_changed(version).is_none());
    }

    #[test]
    fn native_element_tree_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Elements>();
        assert_send_sync::<UiStore>();
    }
}
