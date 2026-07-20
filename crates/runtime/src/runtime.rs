use crate::{event_loop, task::Microtask, Result};
use rquickjs::{
    Array, AsyncContext, AsyncRuntime, CatchResultExt, Ctx, FromJs, Function, Object, Promise,
    ThrowResultExt,
};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug)]
pub enum WindowEventMessage {
    CloseRequested,
    Resized { width: u32, height: u32 },
}

/// A JavaScript execution context that integrates with Tokio.
///
/// The runtime starts rquickjs's driver task when it is created. This is what
/// lets JavaScript promises and Rust futures make progress while the Tokio
/// executor is running.
pub struct Runtime {
    context: AsyncContext,
    _driver: JoinHandle<()>,
}

impl Runtime {
    /// Create a new full JavaScript context.
    ///
    /// This must be called from inside a running Tokio runtime.
    pub async fn new() -> Result<Self> {
        Self::new_with_host(|_| Ok(())).await
    }

    /// Create a context and install application-specific host functions.
    pub async fn new_with_host<F>(installer: F) -> Result<Self>
    where
        F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
    {
        let quickjs = AsyncRuntime::new()?;
        let context = AsyncContext::full(&quickjs).await?;

        event_loop::install(&context).await?;
        context.with(move |ctx| installer(&ctx)).await?;

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
        self.context
            .with(move |ctx| {
                ctx.eval(source)
                    .catch(&ctx)
                    .map_err(|error| {
                        eprintln!("JavaScript evaluation failed: {error}");
                        error
                    })
                    .throw(&ctx)
            })
            .await
    }

    /// Dispatch a batch of native window events to the JavaScript event bridge.
    pub async fn dispatch_window_events(&self, events: &[WindowEventMessage]) -> Result<()> {
        self.context
            .with(move |ctx| {
                let dispatch: Function = ctx.globals().get("__burokku_dispatch_events")?;
                let js_events = Array::new(ctx.clone())?;

                for (index, event) in events.iter().enumerate() {
                    let js_event = Object::new(ctx.clone())?;
                    match event {
                        WindowEventMessage::CloseRequested => {
                            js_event.set("type", "close-requested")?;
                        }
                        WindowEventMessage::Resized { width, height } => {
                            js_event.set("type", "resized")?;
                            js_event.set("width", *width)?;
                            js_event.set("height", *height)?;
                        }
                    }
                    js_events.set(index, js_event)?;
                }

                dispatch.call::<_, ()>((js_events,))
            })
            .await
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
