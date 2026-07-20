use crate::{event_loop, task::Microtask, Result};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, FromJs, Promise, ThrowResultExt};
use tokio::task::JoinHandle;

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

    /// Enqueue native window events as JavaScript macrotasks.
    pub async fn enqueue_window_events(&self, events: &[WindowEventMessage]) -> Result<()> {
        self.context
            .with(move |ctx| event_loop::enqueue_window_events(&ctx, events))
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
