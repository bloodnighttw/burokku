use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock,
};

use super::{Document, DomError, NodeKind};

#[derive(Clone, Debug)]
pub struct DomStore {
    inner: Arc<DomStoreInner>,
}

#[derive(Debug)]
struct DomStoreInner {
    document: RwLock<Document>,
    version: AtomicU64,
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
                document: RwLock::new(Document::new()),
                version: AtomicU64::new(0),
            }),
        }
    }

    pub fn version(&self) -> u64 {
        self.inner.version.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> Document {
        self.inner
            .document
            .read()
            .expect("the DOM document lock is not poisoned")
            .clone()
    }

    pub fn create_node(&self, kind: NodeKind) -> u64 {
        let id = self
            .inner
            .document
            .write()
            .expect("the DOM document lock is not poisoned")
            .create_node(kind);
        self.changed();
        id
    }

    pub fn set_text(&self, id: u64, text: String) -> Result<(), DomError> {
        self.inner
            .document
            .write()
            .expect("the DOM document lock is not poisoned")
            .set_text(id, text)?;
        self.changed();
        Ok(())
    }

    pub fn set_style(&self, id: u64, name: &str, value: Option<&str>) -> Result<(), DomError> {
        self.inner
            .document
            .write()
            .expect("the DOM document lock is not poisoned")
            .set_style(id, name, value)?;
        self.changed();
        Ok(())
    }

    pub fn insert(&self, parent: u64, child: u64, before: Option<u64>) -> Result<(), DomError> {
        self.inner
            .document
            .write()
            .expect("the DOM document lock is not poisoned")
            .insert(parent, child, before)?;
        self.changed();
        Ok(())
    }

    pub fn remove(&self, parent: u64, child: u64) -> Result<(), DomError> {
        self.inner
            .document
            .write()
            .expect("the DOM document lock is not poisoned")
            .remove(parent, child)?;
        self.changed();
        Ok(())
    }

    fn changed(&self) {
        self.inner.version.fetch_add(1, Ordering::AcqRel);
    }
}
