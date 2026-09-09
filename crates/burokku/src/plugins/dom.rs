//! JavaScript DOM plugin backed by Burokku's live UI document.

use std::rc::Rc;

use runtime::{rquickjs::Ctx, Plugin};

#[cfg(test)]
use crate::ui::js_bindings::DomBindingState;
use crate::ui::js_bindings::{self, SharedDomBindings};

/// Installs Burokku's DOM classes and the global `app` document root.
pub struct DomPlugin {
    state: SharedDomBindings,
}

impl DomPlugin {
    /// Creates an independent DOM plugin and backing document.
    pub fn new() -> Self {
        Self {
            state: js_bindings::new_state(),
        }
    }

    pub(crate) fn bindings(&self) -> SharedDomBindings {
        Rc::clone(&self.state)
    }

    #[cfg(test)]
    pub(crate) fn new_with_bindings() -> (Self, SharedDomBindings) {
        let plugin = Self::new();
        let bindings = plugin.bindings();
        (plugin, bindings)
    }

    #[cfg(test)]
    pub(crate) fn reclaim_for_test(&self) -> crate::ui::elements::ReclaimReport {
        self.state
            .try_borrow_mut()
            .expect("DOM plugin state is not borrowed")
            .reclaim_detached()
            .unwrap()
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> std::cell::Ref<'_, DomBindingState> {
        self.state
            .try_borrow()
            .expect("DOM plugin state is not borrowed")
    }
}

impl Default for DomPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for DomPlugin {
    fn name(&self) -> &'static str {
        "burokku-dom"
    }

    fn install<'js>(&self, context: &Ctx<'js>) -> runtime::Result<()> {
        js_bindings::install(context, Rc::clone(&self.state))
    }
}
