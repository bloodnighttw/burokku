use crate::{JsTaskQueue, Plugin, Result};
use rquickjs::{prelude::Func, Ctx, Function, JsLifetime, Object};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};
use tokio::time::{sleep, Duration};

const TIMER_REGISTRY: &str = "__burokku_timers";

#[derive(Clone)]
struct TimerState {
    next_id: Arc<AtomicU32>,
    cancelled: Arc<Mutex<HashSet<u32>>>,
}

// TimerState owns no JavaScript values and does not depend on `'js`.
unsafe impl<'js> JsLifetime<'js> for TimerState {
    type Changed<'to> = TimerState;
}

/// Installs `setTimeout`, `setInterval`, and their cancellation functions.
#[derive(Clone, Copy, Debug, Default)]
pub struct TimersPlugin;

impl Plugin for TimersPlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
        context
            .store_userdata(TimerState {
                next_id: Arc::new(AtomicU32::new(1)),
                cancelled: Arc::new(Mutex::new(HashSet::new())),
            })
            .map_err(|_| rquickjs::Error::Unknown)?;
        context
            .globals()
            .set(TIMER_REGISTRY, Object::new(context.clone())?)?;
        context
            .globals()
            .set("setTimeout", Func::from(set_timeout))?;
        context
            .globals()
            .set("setInterval", Func::from(set_interval))?;
        context
            .globals()
            .set("clearTimeout", Func::from(clear_timer))?;
        context
            .globals()
            .set("clearInterval", Func::from(clear_timer))?;
        Ok(())
    }
}

fn state(context: &Ctx<'_>) -> Result<TimerState> {
    context
        .userdata::<TimerState>()
        .map(|state| state.clone())
        .ok_or(rquickjs::Error::Unknown)
}

fn clear_timer<'js>(context: Ctx<'js>, id: u32) -> Result<()> {
    let state = state(&context)?;
    state
        .cancelled
        .lock()
        .expect("cancelled timer registry is not poisoned")
        .insert(id);
    let timers: Object = context.globals().get(TIMER_REGISTRY)?;
    timers.remove(id)?;
    Ok(())
}

fn set_timeout<'js>(context: Ctx<'js>, callback: Function<'js>, delay: Option<u64>) -> Result<u32> {
    schedule_timer(context, callback, delay.unwrap_or_default(), false)
}

fn set_interval<'js>(
    context: Ctx<'js>,
    callback: Function<'js>,
    delay: Option<u64>,
) -> Result<u32> {
    schedule_timer(context, callback, delay.unwrap_or_default(), true)
}

fn schedule_timer<'js>(
    context: Ctx<'js>,
    callback: Function<'js>,
    delay: u64,
    repeats: bool,
) -> Result<u32> {
    let state = state(&context)?;
    let queue = JsTaskQueue::from_context(&context)?;
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let timers: Object = context.globals().get(TIMER_REGISTRY)?;
    timers.set(id, callback)?;

    context.spawn(async move {
        loop {
            sleep(Duration::from_millis(delay)).await;
            if state
                .cancelled
                .lock()
                .expect("cancelled timer registry is not poisoned")
                .remove(&id)
            {
                break;
            }

            let cancelled = state.cancelled.clone();
            if queue
                .enqueue(move |context| run_timer(context, id, repeats, &cancelled))
                .await
                .is_err()
                || !repeats
            {
                break;
            }
        }
    });

    Ok(id)
}

fn run_timer(
    context: &Ctx<'_>,
    id: u32,
    repeats: bool,
    cancelled: &Mutex<HashSet<u32>>,
) -> Result<()> {
    let timers: Object = context.globals().get(TIMER_REGISTRY)?;
    if cancelled
        .lock()
        .expect("cancelled timer registry is not poisoned")
        .contains(&id)
    {
        timers.remove(id)?;
        return Ok(());
    }

    if let Ok(callback) = timers.get::<_, Function>(id) {
        if !repeats {
            timers.remove(id)?;
        }
        callback.call::<_, ()>(())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{run_timer, TIMER_REGISTRY};
    use rquickjs::{Context, Function, Object, Runtime};
    use std::{collections::HashSet, sync::Mutex};

    #[test]
    fn ignores_cancelled_and_missing_timer_callbacks() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();

        context.with(|context| {
            let timers = Object::new(context.clone()).unwrap();
            let callback: Function = context
                .eval(include_str!("scripts/timer_callback.js"))
                .unwrap();
            timers.set(1, callback).unwrap();
            context.globals().set(TIMER_REGISTRY, timers).unwrap();

            let cancelled = Mutex::new(HashSet::from([1]));
            run_timer(&context, 1, false, &cancelled).unwrap();
            run_timer(&context, 2, false, &Mutex::new(HashSet::new())).unwrap();

            let timer_was_called: bool = context
                .eval(include_str!("scripts/timer_was_called.js"))
                .unwrap();
            assert!(!timer_was_called);
        });
    }
}
