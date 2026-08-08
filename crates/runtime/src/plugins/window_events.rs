use crate::{serializer, MacrotaskQueue, MacrotaskQueueError, Plugin, Result};
use rquickjs::{Ctx, Function};
use serde::Serialize;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModifiersState {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
    pub caps_lock: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WindowEventMessage {
    CloseRequested,
    Resized {
        width: u32,
        height: u32,
    },
    ScaleFactorChanged {
        scale_factor: f64,
        width: u32,
        height: u32,
    },
    Focused(bool),
    Occluded(bool),
    KeyboardInput {
        key_code: u16,
        text: Option<String>,
        state: InputState,
        repeat: bool,
        modifiers: ModifiersState,
    },
    ModifiersChanged(ModifiersState),
    CursorMoved {
        x: f64,
        y: f64,
    },
    MouseInput {
        state: InputState,
        button: MouseButton,
    },
    MouseWheel {
        delta_x: f64,
        delta_y: f64,
        precise: bool,
    },
}

/// Enables native window events to be enqueued as JavaScript macrotasks.
///
/// Clone the plugin before installing it to retain a host-side event handle:
///
/// ```
/// use runtime::{plugins::WindowEventsPlugin, Runtime};
///
/// # async fn example() -> runtime::Result<()> {
/// let window_events = WindowEventsPlugin::default();
/// let runtime = Runtime::builder()
///     .plugin(window_events.clone())
///     .build()
///     .await?;
/// # drop(runtime);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct WindowEventsPlugin {
    queue: Arc<OnceLock<MacrotaskQueue>>,
}

impl Plugin for WindowEventsPlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
        self.queue
            .set(MacrotaskQueue::from_context(context)?)
            .map_err(|_| rquickjs::Error::Unknown)
    }
}

impl WindowEventsPlugin {
    /// Enqueue one native window event, waiting for bounded queue capacity.
    pub async fn enqueue(
        &self,
        event: WindowEventMessage,
    ) -> std::result::Result<(), MacrotaskQueueError> {
        let queue = self.queue.get().ok_or(MacrotaskQueueError::Closed)?;
        queue.enqueue(move |context| dispatch(context, event)).await
    }

    /// Attempt to enqueue one native window event from a synchronous callback.
    ///
    /// A [`MacrotaskQueueError::Full`] result lets the window host coalesce or
    /// drop replaceable events such as cursor movement and resize notifications.
    pub fn try_enqueue(
        &self,
        event: WindowEventMessage,
    ) -> std::result::Result<(), MacrotaskQueueError> {
        let queue = self.queue.get().ok_or(MacrotaskQueueError::Closed)?;
        queue.try_enqueue(move |context| dispatch(context, event))
    }
}

fn dispatch(context: &Ctx<'_>, event: WindowEventMessage) -> Result<()> {
    let Ok(dispatch) = context
        .globals()
        .get::<_, Function>("__burokku_dispatch_event")
    else {
        return Ok(());
    };
    let js_event = serializer::to_object(context, &SerializableWindowEvent::from(&event))
        .map_err(serializer::Error::into_quickjs)?;

    dispatch.call::<_, ()>((js_event,))
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum SerializableWindowEvent<'a> {
    CloseRequested,
    Resized {
        width: u32,
        height: u32,
    },
    ScaleFactorChanged {
        scale_factor: f64,
        width: u32,
        height: u32,
    },
    Focused {
        focused: bool,
    },
    Occluded {
        occluded: bool,
    },
    KeyboardInput {
        key_code: u16,
        text: Option<&'a str>,
        pressed: bool,
        repeat: bool,
        shift_key: bool,
        ctrl_key: bool,
        alt_key: bool,
        meta_key: bool,
        caps_lock: bool,
    },
    ModifiersChanged {
        shift_key: bool,
        ctrl_key: bool,
        alt_key: bool,
        meta_key: bool,
        caps_lock: bool,
    },
    CursorMoved {
        x: f64,
        y: f64,
    },
    MouseInput {
        pressed: bool,
        button: u16,
    },
    MouseWheel {
        delta_x: f64,
        delta_y: f64,
        precise: bool,
    },
}

impl<'a> From<&'a WindowEventMessage> for SerializableWindowEvent<'a> {
    fn from(event: &'a WindowEventMessage) -> Self {
        match event {
            WindowEventMessage::CloseRequested => Self::CloseRequested,
            WindowEventMessage::Resized { width, height } => Self::Resized {
                width: *width,
                height: *height,
            },
            WindowEventMessage::ScaleFactorChanged {
                scale_factor,
                width,
                height,
            } => Self::ScaleFactorChanged {
                scale_factor: *scale_factor,
                width: *width,
                height: *height,
            },
            WindowEventMessage::Focused(focused) => Self::Focused { focused: *focused },
            WindowEventMessage::Occluded(occluded) => Self::Occluded {
                occluded: *occluded,
            },
            WindowEventMessage::KeyboardInput {
                key_code,
                text,
                state,
                repeat,
                modifiers,
            } => Self::KeyboardInput {
                key_code: *key_code,
                text: text.as_deref(),
                pressed: *state == InputState::Pressed,
                repeat: *repeat,
                shift_key: modifiers.shift,
                ctrl_key: modifiers.control,
                alt_key: modifiers.alt,
                meta_key: modifiers.command,
                caps_lock: modifiers.caps_lock,
            },
            WindowEventMessage::ModifiersChanged(modifiers) => Self::ModifiersChanged {
                shift_key: modifiers.shift,
                ctrl_key: modifiers.control,
                alt_key: modifiers.alt,
                meta_key: modifiers.command,
                caps_lock: modifiers.caps_lock,
            },
            WindowEventMessage::CursorMoved { x, y } => Self::CursorMoved { x: *x, y: *y },
            WindowEventMessage::MouseInput { state, button } => Self::MouseInput {
                pressed: *state == InputState::Pressed,
                button: mouse_button_code(*button),
            },
            WindowEventMessage::MouseWheel {
                delta_x,
                delta_y,
                precise,
            } => Self::MouseWheel {
                delta_x: *delta_x,
                delta_y: *delta_y,
                precise: *precise,
            },
        }
    }
}

fn mouse_button_code(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Other(button) => button,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Runtime;

    #[tokio::test(flavor = "current_thread")]
    async fn cloned_plugin_is_a_host_side_event_handle() {
        let window_events = WindowEventsPlugin::default();
        let runtime = Runtime::builder()
            .plugin(window_events.clone())
            .build()
            .await
            .unwrap();
        runtime
            .eval::<()>(
                "globalThis.events = []; \
                 globalThis.__burokku_dispatch_event = event => events.push(event.type)",
            )
            .await
            .unwrap();

        window_events
            .enqueue(WindowEventMessage::Resized {
                width: 800,
                height: 600,
            })
            .await
            .unwrap();
        window_events
            .enqueue(WindowEventMessage::CloseRequested)
            .await
            .unwrap();

        let event_types: Vec<String> = runtime.eval("events").await.unwrap();
        assert_eq!(event_types, ["resized", "close-requested"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn serializes_window_event_fields_to_the_existing_js_shape() {
        let window_events = WindowEventsPlugin::default();
        let runtime = Runtime::builder()
            .plugin(window_events.clone())
            .build()
            .await
            .unwrap();
        runtime
            .eval::<()>(
                "globalThis.__burokku_dispatch_event = event => globalThis.lastEvent = event",
            )
            .await
            .unwrap();

        window_events
            .enqueue(WindowEventMessage::KeyboardInput {
                key_code: 42,
                text: None,
                state: InputState::Pressed,
                repeat: true,
                modifiers: ModifiersState {
                    shift: true,
                    control: false,
                    alt: true,
                    command: false,
                    caps_lock: true,
                },
            })
            .await
            .unwrap();

        let has_expected_shape: bool = runtime
            .eval(
                "lastEvent.type === 'keyboard-input' && \
                 lastEvent.keyCode === 42 && \
                 lastEvent.text === undefined && \
                 lastEvent.pressed === true && \
                 lastEvent.repeat === true && \
                 lastEvent.shiftKey === true && \
                 lastEvent.ctrlKey === false && \
                 lastEvent.altKey === true && \
                 lastEvent.metaKey === false && \
                 lastEvent.capsLock === true",
            )
            .await
            .unwrap();
        assert!(has_expected_shape);
    }
}
