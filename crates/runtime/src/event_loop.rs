use crate::{
    host, task::Macrotask, InputState, ModifiersState, MouseButton, Result, WindowEventMessage,
};
use rquickjs::{AsyncContext, Ctx, Function, JsLifetime, Object};
use std::{
    collections::HashSet,
    sync::{atomic::AtomicU32, Arc, Mutex},
};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::time::{sleep, Duration};

pub(crate) const TIMER_REGISTRY: &str = "__burokku_timers";

pub(crate) struct TimerTask {
    pub(crate) id: u32,
    pub(crate) repeats: bool,
}

pub(crate) enum MacrotaskMessage {
    Timer(TimerTask),
    WindowEvent(WindowEventMessage),
}

#[derive(Clone)]
pub(crate) struct EventLoopState {
    pub(crate) tasks: tokio::sync::mpsc::UnboundedSender<MacrotaskMessage>,
    pub(crate) next_timer_id: Arc<AtomicU32>,
    pub(crate) cancelled_timers: Arc<Mutex<HashSet<u32>>>,
}

// This state contains no JavaScript values; it is safe to use for every
// JavaScript lifetime while it remains owned by the QuickJS runtime userdata.
unsafe impl<'js> JsLifetime<'js> for EventLoopState {
    type Changed<'to> = EventLoopState;
}

pub(crate) async fn install(context: &AsyncContext) -> Result<()> {
    let (macrotask_sender, macrotask_receiver) = mpsc::unbounded_channel();
    let event_loop = EventLoopState {
        tasks: macrotask_sender,
        next_timer_id: Arc::new(AtomicU32::new(1)),
        cancelled_timers: Arc::new(Mutex::new(HashSet::new())),
    };

    context
        .with(move |ctx| -> Result<()> {
            ctx.store_userdata(event_loop.clone())
                .map_err(|_| rquickjs::Error::Unknown)?;
            ctx.globals()
                .set(TIMER_REGISTRY, Object::new(ctx.clone())?)?;
            host::install(&ctx)?;

            ctx.spawn(Macrotask::new(run_macrotasks(
                ctx.clone(),
                macrotask_receiver,
            )));
            Ok(())
        })
        .await
}

async fn run_macrotasks<'js>(context: Ctx<'js>, mut tasks: UnboundedReceiver<MacrotaskMessage>) {
    let timers: Object = context
        .globals()
        .get(TIMER_REGISTRY)
        .expect("timer registry is installed before the event loop starts");

    while let Some(task) = tasks.recv().await {
        match task {
            MacrotaskMessage::Timer(task) => {
                if context.userdata::<EventLoopState>().is_some_and(|state| {
                    state
                        .cancelled_timers
                        .lock()
                        .expect("cancelled timer registry is not poisoned")
                        .contains(&task.id)
                }) {
                    let _ = timers.remove(task.id);
                    continue;
                }
                if let Ok(callback) = timers.get::<_, Function>(task.id) {
                    if !task.repeats {
                        let _ = timers.remove(task.id);
                    }
                    let _ = callback.call::<_, ()>(());
                }
            }
            MacrotaskMessage::WindowEvent(event) => dispatch_window_event(&context, event),
        }

        // Give QuickJS a chance to drain promise jobs before the next timer.
        sleep(Duration::from_millis(0)).await;
    }
}

fn dispatch_window_event<'js>(context: &Ctx<'js>, event: WindowEventMessage) {
    let Ok(dispatch) = context
        .globals()
        .get::<_, Function>("__burokku_dispatch_event")
    else {
        return;
    };
    let Ok(js_event) = Object::new(context.clone()) else {
        return;
    };

    let result = match event {
        WindowEventMessage::CloseRequested => js_event.set("type", "close-requested"),
        WindowEventMessage::Resized { width, height } => js_event
            .set("type", "resized")
            .and_then(|()| js_event.set("width", width))
            .and_then(|()| js_event.set("height", height)),
        WindowEventMessage::ScaleFactorChanged {
            scale_factor,
            width,
            height,
        } => js_event
            .set("type", "scale-factor-changed")
            .and_then(|()| js_event.set("scaleFactor", scale_factor))
            .and_then(|()| js_event.set("width", width))
            .and_then(|()| js_event.set("height", height)),
        WindowEventMessage::Focused(focused) => js_event
            .set("type", "focused")
            .and_then(|()| js_event.set("focused", focused)),
        WindowEventMessage::Occluded(occluded) => js_event
            .set("type", "occluded")
            .and_then(|()| js_event.set("occluded", occluded)),
        WindowEventMessage::KeyboardInput {
            key_code,
            text,
            state,
            repeat,
            modifiers,
        } => js_event
            .set("type", "keyboard-input")
            .and_then(|()| js_event.set("keyCode", key_code))
            .and_then(|()| js_event.set("text", text))
            .and_then(|()| js_event.set("pressed", state == InputState::Pressed))
            .and_then(|()| js_event.set("repeat", repeat))
            .and_then(|()| set_modifiers(&js_event, modifiers)),
        WindowEventMessage::ModifiersChanged(modifiers) => js_event
            .set("type", "modifiers-changed")
            .and_then(|()| set_modifiers(&js_event, modifiers)),
        WindowEventMessage::CursorMoved { x, y } => js_event
            .set("type", "cursor-moved")
            .and_then(|()| js_event.set("x", x))
            .and_then(|()| js_event.set("y", y)),
        WindowEventMessage::MouseInput { state, button } => js_event
            .set("type", "mouse-input")
            .and_then(|()| js_event.set("pressed", state == InputState::Pressed))
            .and_then(|()| js_event.set("button", mouse_button_code(button))),
        WindowEventMessage::MouseWheel {
            delta_x,
            delta_y,
            precise,
        } => js_event
            .set("type", "mouse-wheel")
            .and_then(|()| js_event.set("deltaX", delta_x))
            .and_then(|()| js_event.set("deltaY", delta_y))
            .and_then(|()| js_event.set("precise", precise)),
    };
    if result.is_ok() {
        let _ = dispatch.call::<_, ()>((js_event,));
    }
}

fn set_modifiers<'js>(event: &Object<'js>, modifiers: ModifiersState) -> Result<()> {
    event
        .set("shiftKey", modifiers.shift)
        .and_then(|()| event.set("ctrlKey", modifiers.control))
        .and_then(|()| event.set("altKey", modifiers.alt))
        .and_then(|()| event.set("metaKey", modifiers.command))
        .and_then(|()| event.set("capsLock", modifiers.caps_lock))
}

fn mouse_button_code(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Other(button) => button,
    }
}

pub(crate) fn state<'js>(context: &Ctx<'js>) -> Result<EventLoopState> {
    context
        .userdata::<EventLoopState>()
        .ok_or(rquickjs::Error::Unknown)
        .map(|state| state.clone())
}

pub(crate) fn enqueue_window_events<'js>(
    context: &Ctx<'js>,
    events: &[WindowEventMessage],
) -> Result<()> {
    let state = state(context)?;
    for event in events {
        state
            .tasks
            .send(MacrotaskMessage::WindowEvent(event.clone()))
            .map_err(|_| rquickjs::Error::Unknown)?;
    }
    Ok(())
}
