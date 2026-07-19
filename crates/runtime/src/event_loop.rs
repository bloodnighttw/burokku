use crate::{host, task::Macrotask, Result};
use rquickjs::{AsyncContext, Ctx, JsLifetime, Object};
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

#[derive(Clone)]
pub(crate) struct EventLoopState {
    pub(crate) tasks: tokio::sync::mpsc::UnboundedSender<TimerTask>,
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

async fn run_macrotasks<'js>(context: Ctx<'js>, mut tasks: UnboundedReceiver<TimerTask>) {
    let timers: Object = context
        .globals()
        .get(TIMER_REGISTRY)
        .expect("timer registry is installed before the event loop starts");

    while let Some(task) = tasks.recv().await {
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
        if let Ok(callback) = timers.get::<_, rquickjs::Function>(task.id) {
            if !task.repeats {
                let _ = timers.remove(task.id);
            }
            let _ = callback.call::<_, ()>(());
        }

        // Give QuickJS a chance to drain promise jobs before the next timer.
        sleep(Duration::from_millis(0)).await;
    }
}

pub(crate) fn state<'js>(context: &Ctx<'js>) -> Result<EventLoopState> {
    context
        .userdata::<EventLoopState>()
        .ok_or(rquickjs::Error::Unknown)
        .map(|state| state.clone())
}
