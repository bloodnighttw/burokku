use crate::{MacrotaskQueue, Plugin, Result};
use rquickjs::{Ctx, Function, Object};
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
    /// Enqueue native window events into the runtime where this plugin was installed.
    pub fn enqueue(&self, events: &[WindowEventMessage]) -> Result<()> {
        let queue = self.queue.get().ok_or(rquickjs::Error::Unknown)?;
        for event in events.iter().cloned() {
            queue.enqueue(move |context| dispatch(context, event))?;
        }
        Ok(())
    }
}

fn dispatch(context: &Ctx<'_>, event: WindowEventMessage) -> Result<()> {
    let Ok(dispatch) = context
        .globals()
        .get::<_, Function>("__burokku_dispatch_event")
    else {
        return Ok(());
    };
    let js_event = Object::new(context.clone())?;

    match event {
        WindowEventMessage::CloseRequested => js_event.set("type", "close-requested")?,
        WindowEventMessage::Resized { width, height } => {
            js_event.set("type", "resized")?;
            js_event.set("width", width)?;
            js_event.set("height", height)?;
        }
        WindowEventMessage::ScaleFactorChanged {
            scale_factor,
            width,
            height,
        } => {
            js_event.set("type", "scale-factor-changed")?;
            js_event.set("scaleFactor", scale_factor)?;
            js_event.set("width", width)?;
            js_event.set("height", height)?;
        }
        WindowEventMessage::Focused(focused) => {
            js_event.set("type", "focused")?;
            js_event.set("focused", focused)?;
        }
        WindowEventMessage::Occluded(occluded) => {
            js_event.set("type", "occluded")?;
            js_event.set("occluded", occluded)?;
        }
        WindowEventMessage::KeyboardInput {
            key_code,
            text,
            state,
            repeat,
            modifiers,
        } => {
            js_event.set("type", "keyboard-input")?;
            js_event.set("keyCode", key_code)?;
            js_event.set("text", text)?;
            js_event.set("pressed", state == InputState::Pressed)?;
            js_event.set("repeat", repeat)?;
            set_modifiers(&js_event, modifiers)?;
        }
        WindowEventMessage::ModifiersChanged(modifiers) => {
            js_event.set("type", "modifiers-changed")?;
            set_modifiers(&js_event, modifiers)?;
        }
        WindowEventMessage::CursorMoved { x, y } => {
            js_event.set("type", "cursor-moved")?;
            js_event.set("x", x)?;
            js_event.set("y", y)?;
        }
        WindowEventMessage::MouseInput { state, button } => {
            js_event.set("type", "mouse-input")?;
            js_event.set("pressed", state == InputState::Pressed)?;
            js_event.set("button", mouse_button_code(button))?;
        }
        WindowEventMessage::MouseWheel {
            delta_x,
            delta_y,
            precise,
        } => {
            js_event.set("type", "mouse-wheel")?;
            js_event.set("deltaX", delta_x)?;
            js_event.set("deltaY", delta_y)?;
            js_event.set("precise", precise)?;
        }
    }

    dispatch.call::<_, ()>((js_event,))
}

fn set_modifiers(event: &Object<'_>, modifiers: ModifiersState) -> Result<()> {
    event.set("shiftKey", modifiers.shift)?;
    event.set("ctrlKey", modifiers.control)?;
    event.set("altKey", modifiers.alt)?;
    event.set("metaKey", modifiers.command)?;
    event.set("capsLock", modifiers.caps_lock)
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
            .enqueue(&[
                WindowEventMessage::Resized {
                    width: 800,
                    height: 600,
                },
                WindowEventMessage::CloseRequested,
            ])
            .unwrap();

        let event_types: Vec<String> = runtime.eval("events").await.unwrap();
        assert_eq!(event_types, ["resized", "close-requested"]);
    }
}
