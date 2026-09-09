//! Event-listener registration and lookup.

use std::collections::HashMap;

use rquickjs::{class::Trace, Function, JsLifetime};

#[derive(Clone, Trace, JsLifetime)]
pub(super) struct EventListener<'js> {
    pub(super) id: u64,
    pub(super) callback: Function<'js>,
}

#[derive(Default, Trace, JsLifetime)]
pub(crate) struct ListenerRegistry<'js> {
    by_type: HashMap<String, Vec<EventListener<'js>>>,
    next_id: u64,
}

impl<'js> ListenerRegistry<'js> {
    pub(in crate::ui::js_bindings) fn add(
        &mut self,
        event_type: String,
        callback: Function<'js>,
    ) -> bool {
        if self
            .by_type
            .get(&event_type)
            .is_some_and(|listeners| listeners.iter().any(|item| item.callback == callback))
        {
            return false;
        }

        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("event listener IDs exhausted");
        self.by_type
            .entry(event_type)
            .or_default()
            .push(EventListener { id, callback });
        true
    }

    pub(in crate::ui::js_bindings) fn remove(
        &mut self,
        event_type: &str,
        callback: &Function<'js>,
    ) -> bool {
        let Some(listeners) = self.by_type.get_mut(event_type) else {
            return false;
        };
        let previous_len = listeners.len();
        listeners.retain(|candidate| candidate.callback != *callback);
        let removed = listeners.len() != previous_len;
        if listeners.is_empty() {
            self.by_type.remove(event_type);
        }
        removed
    }

    pub(super) fn matching(&self, event_type: &str) -> Vec<EventListener<'js>> {
        self.by_type.get(event_type).cloned().unwrap_or_default()
    }

    pub(super) fn contains(&self, event_type: &str, listener_id: u64) -> bool {
        self.by_type
            .get(event_type)
            .is_some_and(|listeners| listeners.iter().any(|item| item.id == listener_id))
    }

    pub(in crate::ui::js_bindings) fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }
}
