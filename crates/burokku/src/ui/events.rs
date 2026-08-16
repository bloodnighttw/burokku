use runtime::{
    rquickjs::{Ctx, Function, Object},
    MacrotaskQueue, MacrotaskQueueError, Result as RuntimeResult,
};
use slotmap::Key;

use super::elements::NodeId;

const NATIVE_EVENT_DISPATCH: &str = "__burokkuDispatchNativeEvent";

/// Modifier state copied from a native event before it crosses to BTS.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EventModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
    pub caps_lock: bool,
}

/// An owned DOM event payload suitable for the bounded BTS macrotask queue.
#[derive(Clone, Debug, PartialEq)]
pub enum DomEventData {
    Pointer {
        event_type: &'static str,
        client_x: f64,
        client_y: f64,
        button: i16,
        buttons: u16,
        modifiers: EventModifiers,
    },
    Wheel {
        client_x: f64,
        client_y: f64,
        delta_x: f64,
        delta_y: f64,
        precise: bool,
        modifiers: EventModifiers,
    },
    Keyboard {
        event_type: &'static str,
        key_code: u16,
        key: Option<String>,
        repeat: bool,
        modifiers: EventModifiers,
    },
    Focus {
        focused: bool,
    },
}

impl DomEventData {
    fn event_type(&self) -> &'static str {
        match self {
            Self::Pointer { event_type, .. } | Self::Keyboard { event_type, .. } => event_type,
            Self::Wheel { .. } => "wheel",
            Self::Focus { focused: true } => "focus",
            Self::Focus { focused: false } => "blur",
        }
    }

    fn bubbles(&self) -> bool {
        !matches!(self, Self::Focus { .. })
    }
}

/// A native event targeted against one completely presented DOM revision.
#[derive(Clone, Debug, PartialEq)]
pub struct DomEvent {
    pub target: NodeId,
    pub presented_revision: u64,
    pub data: DomEventData,
}

/// Result of attempting to submit an event without blocking MTS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    Queued,
    /// The bounded BTS queue was full. The newest event is dropped.
    DroppedBackpressure,
    /// BTS has shut down and no longer accepts native events.
    RuntimeClosed,
}

/// Sends native UI events directly to BTS's bounded macrotask queue.
///
/// Native callbacks never wait for JavaScript. Queue saturation drops the
/// newest event, while a closed queue tells the caller to begin shutdown.
#[derive(Clone, Debug)]
pub struct EventDispatcher {
    queue: MacrotaskQueue,
}

impl EventDispatcher {
    pub fn new(queue: MacrotaskQueue) -> Self {
        Self { queue }
    }

    pub fn try_dispatch(&self, event: DomEvent) -> DispatchOutcome {
        match self
            .queue
            .try_enqueue(move |context| dispatch_event(context, event))
        {
            Ok(()) => DispatchOutcome::Queued,
            Err(MacrotaskQueueError::Full) => DispatchOutcome::DroppedBackpressure,
            Err(MacrotaskQueueError::Closed) => DispatchOutcome::RuntimeClosed,
        }
    }
}

fn dispatch_event(context: &Ctx<'_>, event: DomEvent) -> RuntimeResult<()> {
    let dispatch: Function = context.globals().get(NATIVE_EVENT_DISPATCH)?;
    let init = Object::new(context.clone())?;
    init.set("type", event.data.event_type())?;
    init.set("bubbles", event.data.bubbles())?;
    init.set("cancelable", true)?;
    // A string preserves all u64 revisions without JavaScript Number rounding.
    init.set("presentedRevision", event.presented_revision.to_string())?;

    match event.data {
        DomEventData::Pointer {
            client_x,
            client_y,
            button,
            buttons,
            modifiers,
            ..
        } => {
            init.set("clientX", client_x)?;
            init.set("clientY", client_y)?;
            init.set("button", button)?;
            init.set("buttons", buttons)?;
            set_modifiers(&init, modifiers)?;
        }
        DomEventData::Wheel {
            client_x,
            client_y,
            delta_x,
            delta_y,
            precise,
            modifiers,
        } => {
            init.set("clientX", client_x)?;
            init.set("clientY", client_y)?;
            init.set("deltaX", delta_x)?;
            init.set("deltaY", delta_y)?;
            init.set("deltaMode", if precise { 0 } else { 1 })?;
            set_modifiers(&init, modifiers)?;
        }
        DomEventData::Keyboard {
            key_code,
            key,
            repeat,
            modifiers,
            ..
        } => {
            init.set("keyCode", key_code)?;
            init.set("which", key_code)?;
            init.set("key", key.unwrap_or_default())?;
            init.set("repeat", repeat)?;
            set_modifiers(&init, modifiers)?;
        }
        DomEventData::Focus { .. } => {}
    }

    let handle = event.target.data().as_ffi().to_string();
    let _: bool = dispatch.call((handle, init))?;
    Ok(())
}

fn set_modifiers(init: &Object<'_>, modifiers: EventModifiers) -> RuntimeResult<()> {
    init.set("shiftKey", modifiers.shift)?;
    init.set("ctrlKey", modifiers.control)?;
    init.set("altKey", modifiers.alt)?;
    init.set("metaKey", modifiers.command)?;
    init.set("capsLock", modifiers.caps_lock)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::js_bridge::DomPlugin;
    use runtime::{Runtime, RuntimeRole};

    fn pointer_event(target: NodeId, revision: u64) -> DomEvent {
        DomEvent {
            target,
            presented_revision: revision,
            data: DomEventData::Pointer {
                event_type: "mousedown",
                client_x: 12.5,
                client_y: 24.0,
                button: 0,
                buttons: 1,
                modifiers: EventModifiers {
                    shift: true,
                    ..EventModifiers::default()
                },
            },
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatches_owned_events_to_the_stable_javascript_node() {
        let (plugin, shared) = DomPlugin::with_new_dom();
        let runtime = Runtime::builder()
            .role(RuntimeRole::Background)
            .plugin(plugin)
            .build()
            .await
            .unwrap();
        runtime
            .eval::<()>(
                r#"
                globalThis.receivedNativeEvent = null;
                const target = document.createElement("div");
                target.addEventListener("mousedown", event => {
                    receivedNativeEvent = [
                        event.type,
                        event.presentedRevision,
                        String(event.clientX),
                        String(event.clientY),
                        String(event.button),
                        String(event.buttons),
                        String(event.shiftKey),
                        event.target.nodeName,
                        event.currentTarget.nodeName
                    ];
                });
                document.body.addEventListener("mousedown", event => {
                    receivedNativeEvent.push(event.target.nodeName, event.currentTarget.nodeName);
                });
                document.addEventListener("mousedown", event => {
                    receivedNativeEvent.push(event.currentTarget.nodeName, String(event.composedPath().length));
                });
                document.body.appendChild(target);
                "#,
            )
            .await
            .unwrap();

        let snapshot = shared.load();
        let body = snapshot.dom().children(snapshot.dom().root()).unwrap()[0];
        let target = snapshot.dom().children(body).unwrap()[0];
        let dispatcher = EventDispatcher::new(runtime.macrotask_queue());
        assert_eq!(
            dispatcher.try_dispatch(pointer_event(target, snapshot.revision())),
            DispatchOutcome::Queued
        );

        let received: Vec<String> = runtime.eval("receivedNativeEvent").await.unwrap();
        assert_eq!(
            received,
            vec![
                "mousedown".to_owned(),
                snapshot.revision().to_string(),
                "12.5".to_owned(),
                "24".to_owned(),
                "0".to_owned(),
                "1".to_owned(),
                "true".to_owned(),
                "DIV".to_owned(),
                "DIV".to_owned(),
                "DIV".to_owned(),
                "WINDOW".to_owned(),
                "#document".to_owned(),
                "4".to_owned(),
            ]
        );
        runtime.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stale_targets_are_quietly_dropped_before_javascript_dispatch() {
        let (plugin, _shared) = DomPlugin::with_new_dom();
        let runtime = Runtime::builder()
            .role(RuntimeRole::Background)
            .plugin(plugin)
            .build()
            .await
            .unwrap();
        runtime
            .eval::<()>(
                r#"
                globalThis.nativeEventCount = 0;
                document.body.addEventListener("mousedown", () => nativeEventCount++);
                "#,
            )
            .await
            .unwrap();

        let dispatcher = EventDispatcher::new(runtime.macrotask_queue());
        assert_eq!(
            dispatcher.try_dispatch(pointer_event(NodeId::null(), 1)),
            DispatchOutcome::Queued
        );
        let count: u32 = runtime.eval("nativeEventCount").await.unwrap();
        assert_eq!(count, 0);
        runtime.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn full_and_closed_queues_have_explicit_nonblocking_outcomes() {
        let (plugin, shared) = DomPlugin::with_new_dom();
        let (runtime, driver) = Runtime::builder()
            .role(RuntimeRole::Background)
            .macrotask_capacity(1)
            .plugin(plugin)
            .build_driven()
            .await
            .unwrap();
        let target = shared.load().dom().root();
        let dispatcher = EventDispatcher::new(runtime.macrotask_queue());

        assert_eq!(
            dispatcher.try_dispatch(pointer_event(target, 0)),
            DispatchOutcome::Queued
        );
        assert_eq!(
            dispatcher.try_dispatch(pointer_event(target, 0)),
            DispatchOutcome::DroppedBackpressure
        );

        drop(runtime);
        driver.run().await;
        assert_eq!(
            dispatcher.try_dispatch(pointer_event(target, 0)),
            DispatchOutcome::RuntimeClosed
        );
    }
}
