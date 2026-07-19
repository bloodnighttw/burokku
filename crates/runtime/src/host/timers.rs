use crate::{
    event_loop::{self, TimerTask},
    task::Macrotask,
    Result,
};
use rquickjs::{prelude::Func, Ctx, Function, Object};
use tokio::time::{sleep, Duration};

pub(crate) fn install<'js>(context: &Ctx<'js>) -> Result<()> {
    context
        .globals()
        .set("setTimeout", Func::from(set_timeout))?;
    context
        .globals()
        .set("setInterval", Func::from(set_interval))?;
    Ok(())
}

fn set_timeout<'js>(context: Ctx<'js>, callback: Function<'js>, delay: Option<u64>) -> Result<u32> {
    let state = event_loop::state(&context)?;
    let delay = delay.unwrap_or_default();
    let id = state
        .next_timer_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let timers: Object = context.globals().get(event_loop::TIMER_REGISTRY)?;
    timers.set(id, callback)?;

    context.spawn(Macrotask::new(async move {
        sleep(Duration::from_millis(delay)).await;
        let _ = state.tasks.send(TimerTask { id, repeats: false });
    }));

    Ok(id)
}

fn set_interval<'js>(
    context: Ctx<'js>,
    callback: Function<'js>,
    delay: Option<u64>,
) -> Result<u32> {
    let state = event_loop::state(&context)?;
    let delay = delay.unwrap_or_default();
    let id = state
        .next_timer_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let timers: Object = context.globals().get(event_loop::TIMER_REGISTRY)?;
    timers.set(id, callback)?;

    context.spawn(Macrotask::new(async move {
        loop {
            sleep(Duration::from_millis(delay)).await;
            if state.tasks.send(TimerTask { id, repeats: true }).is_err() {
                break;
            }
        }
    }));

    Ok(id)
}
