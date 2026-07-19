//! An asynchronous JavaScript runtime backed by rquickjs and Tokio.

mod task;

use rquickjs::{
    prelude::Func, AsyncContext, AsyncRuntime, Ctx, FromJs, Function, JsLifetime, Object, Promise,
};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

use task::{Macrotask, Microtask};

pub use rquickjs::Error;

/// The result type returned by this crate.
pub type Result<T> = rquickjs::Result<T>;

/// A JavaScript execution context that integrates with Tokio.
///
/// The runtime starts rquickjs's driver task when it is created. This is what
/// lets JavaScript promises and Rust futures make progress while the Tokio
/// executor is running.
pub struct Runtime {
    context: AsyncContext,
    _driver: JoinHandle<()>,
}

#[derive(Clone)]
struct EventLoopState {
    tasks: tokio::sync::mpsc::UnboundedSender<u32>,
    next_timer_id: Arc<AtomicU32>,
}

// This state contains no JavaScript values; it is safe to use for every
// JavaScript lifetime while it remains owned by the QuickJS runtime userdata.
unsafe impl<'js> JsLifetime<'js> for EventLoopState {
    type Changed<'to> = EventLoopState;
}

async fn run_macrotasks<'js>(context: Ctx<'js>, mut tasks: UnboundedReceiver<u32>) {
    let timers: Object = context
        .globals()
        .get("__burokku_timers")
        .expect("timer registry is installed before the event loop starts");

    while let Some(id) = tasks.recv().await {
        if let Ok(callback) = timers.get::<_, Function>(id) {
            let _ = timers.remove(id);
            let _ = callback.call::<_, ()>(());
        }

        // Do not let multiple ready macrotasks run in one scheduler poll. The
        // QuickJS driver gets to drain promise jobs before the next timer.
        sleep(Duration::from_millis(0)).await;
    }
}

fn set_timeout<'js>(context: Ctx<'js>, callback: Function<'js>, delay: Option<u64>) -> Result<u32> {
    let state = context
        .userdata::<EventLoopState>()
        .ok_or(rquickjs::Error::Unknown)?
        .clone();
    let delay = delay.unwrap_or_default();
    let id = state.next_timer_id.fetch_add(1, Ordering::Relaxed);
    let timers: Object = context.globals().get("__burokku_timers")?;
    timers.set(id, callback)?;

    context.spawn(Macrotask::new(async move {
        sleep(Duration::from_millis(delay)).await;
        let _ = state.tasks.send(id);
    }));

    Ok(id)
}

impl Runtime {
    /// Create a new full JavaScript context.
    ///
    /// This must be called from inside a running Tokio runtime.
    pub async fn new() -> Result<Self> {
        let quickjs = AsyncRuntime::new()?;
        let context = AsyncContext::full(&quickjs).await?;
        let (macrotask_sender, macrotask_receiver) = mpsc::unbounded_channel();
        let event_loop = EventLoopState {
            tasks: macrotask_sender,
            next_timer_id: Arc::new(AtomicU32::new(1)),
        };

        context
            .with(move |ctx| -> Result<()> {
                ctx.store_userdata(event_loop.clone())
                    .map_err(|_| rquickjs::Error::Unknown)?;
                ctx.globals()
                    .set("__burokku_timers", Object::new(ctx.clone())?)?;
                ctx.globals().set("setTimeout", Func::from(set_timeout))?;

                let console = Object::new(ctx.clone())?;
                console.set(
                    "log",
                    Func::from(|message: String| {
                        println!("{message}");
                    }),
                )?;
                ctx.globals().set("console", console)?;

                ctx.spawn(Macrotask::new(run_macrotasks(
                    ctx.clone(),
                    macrotask_receiver,
                )));
                Ok(())
            })
            .await?;

        let driver = tokio::spawn(context.runtime().drive());

        Ok(Self {
            context,
            _driver: driver,
        })
    }

    /// Evaluate a synchronous JavaScript expression or script.
    pub async fn eval<T>(&self, source: impl Into<Vec<u8>>) -> Result<T>
    where
        for<'js> T: FromJs<'js> + Send,
    {
        let source = source.into();
        self.context.with(move |ctx| ctx.eval(source)).await
    }

    /// Evaluate a JavaScript expression that returns a promise.
    ///
    /// The QuickJS job queue is driven while the promise is resolved, so this
    /// also handles JavaScript promises created by the evaluated script.
    pub async fn eval_promise<T>(&self, source: impl Into<Vec<u8>>) -> Result<T>
    where
        for<'js> T: FromJs<'js> + Send + 'static,
    {
        let source = source.into();
        rquickjs::async_with!(self.context => |ctx| {
            let promise: Promise = ctx.eval(source)?;
            Microtask::new(promise.into_future::<T>()).await
        })
        .await
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Runtime").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::Runtime;

    #[tokio::test(flavor = "current_thread")]
    async fn evaluates_javascript() {
        let runtime = Runtime::new().await.unwrap();
        let value: i32 = runtime.eval("20 + 22").await.unwrap();

        assert_eq!(value, 42);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolves_a_promise() {
        let runtime = Runtime::new().await.unwrap();
        let value: String = runtime
            .eval_promise("Promise.resolve('Burokku')")
            .await
            .unwrap();

        assert_eq!(value, "Burokku");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn set_timeout_is_a_macrotask_and_promises_are_microtasks() {
        let runtime = Runtime::new().await.unwrap();
        let value: String = runtime
            .eval_promise(
                r#"
                (async () => {
                    const order = [];
                    setTimeout(() => {
                        order.push("macrotask-1");
                        Promise.resolve().then(() => order.push("microtask"));
                    }, 10);
                    setTimeout(() => order.push("macrotask-2"), 10);
                    await new Promise(resolve => setTimeout(resolve, 20));
                    return order.join(",");
                })()
                "#,
            )
            .await
            .unwrap();

        assert_eq!(value, "macrotask-1,microtask,macrotask-2");
    }
}
