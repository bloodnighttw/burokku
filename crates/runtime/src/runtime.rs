use crate::{event_loop, task::Microtask, Result};
use rquickjs::{AsyncContext, AsyncRuntime, FromJs, Promise};
use tokio::task::JoinHandle;

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
        let quickjs = AsyncRuntime::new()?;
        let context = AsyncContext::full(&quickjs).await?;

        event_loop::install(&context).await?;

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
