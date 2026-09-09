//! Queue boundary between native host input and DOM event dispatch.

mod keyboard;
mod mouse;
mod pointer;

use runtime::JsTaskQueueError;

use super::DomBindingState;
use crate::ui::{
    events::DomKeyboardEvent,
    host::{NativeKeyboardEvent, NativeMouseInput},
};

#[cfg(test)]
pub(super) use mouse::execute_mouse_event;

impl DomBindingState {
    pub(crate) fn enqueue_pointer_cancel(&self) -> std::result::Result<(), JsTaskQueueError> {
        self.task_queue
            .as_ref()
            .ok_or(JsTaskQueueError::Closed)?
            .try_enqueue(pointer::execute_pointer_cancel)
    }

    pub(crate) fn enqueue_pointer_cancel_when_ready(
        &self,
    ) -> std::result::Result<(), JsTaskQueueError> {
        let queue = self
            .task_queue
            .as_ref()
            .ok_or(JsTaskQueueError::Closed)?
            .clone();
        tokio::task::spawn_local(async move {
            if let Err(error) = queue.enqueue(pointer::execute_pointer_cancel).await {
                eprintln!("Burokku warning: pointer cancellation stopped: {error}");
            }
        });
        Ok(())
    }

    pub(crate) fn enqueue_mouse_input(
        &self,
        input: NativeMouseInput,
    ) -> std::result::Result<(), JsTaskQueueError> {
        self.task_queue
            .as_ref()
            .ok_or(JsTaskQueueError::Closed)?
            .try_enqueue(move |context| pointer::execute_mouse_input(context, input))
    }

    pub(crate) fn enqueue_keyboard_event(
        &self,
        event: NativeKeyboardEvent,
    ) -> std::result::Result<(), JsTaskQueueError> {
        let event = DomKeyboardEvent::from(event);
        self.task_queue
            .as_ref()
            .ok_or(JsTaskQueueError::Closed)?
            .try_enqueue(move |context| keyboard::execute_keyboard_event(context, event))
    }
}
