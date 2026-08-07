use crate::{event_loop, plugins, MacrotaskQueue, Result, RuntimeBuilder, WindowEventMessage};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, FromJs, Promise, ThrowResultExt};
use tokio::task::JoinHandle;

/// A JavaScript execution context that integrates QuickJS with Tokio.
///
/// Every runtime has a generic macrotask queue. QuickJS's own job queue is
/// drained as the microtask checkpoint after each macrotask. Host APIs are
/// supplied by composable [`crate::Plugin`] implementations.
pub struct Runtime {
    context: AsyncContext,
    macrotasks: MacrotaskQueue,
    _driver: JoinHandle<()>,
}

impl Runtime {
    /// Start configuring a runtime with its standard plugins.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    /// Create a runtime with console, timer, and window-event plugins.
    ///
    /// This must be called from inside a running Tokio runtime.
    pub async fn new() -> Result<Self> {
        Self::builder().build().await
    }

    /// Create a standard runtime and install one application-specific host.
    ///
    /// New integrations should generally implement [`crate::Plugin`] and use
    /// [`Runtime::builder`]. This method remains for source compatibility with
    /// small, one-off installers.
    pub async fn new_with_host<F>(installer: F) -> Result<Self>
    where
        F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
    {
        Self::build(RuntimeBuilder::new(), installer).await
    }

    pub(crate) async fn build<F>(builder: RuntimeBuilder, installer: F) -> Result<Self>
    where
        F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
    {
        let quickjs = AsyncRuntime::new()?;
        let context = AsyncContext::full(&quickjs).await?;

        let macrotasks = event_loop::install(&context).await?;
        context
            .with(move |context| {
                for plugin in builder.plugins {
                    plugin.install(&context)?;
                }
                installer(&context)
            })
            .await?;

        let driver = tokio::spawn(context.runtime().drive());

        Ok(Self {
            context,
            macrotasks,
            _driver: driver,
        })
    }

    /// Clone a handle that can enqueue native macrotasks in this runtime.
    pub fn macrotask_queue(&self) -> MacrotaskQueue {
        self.macrotasks.clone()
    }

    /// Evaluate a synchronous JavaScript expression or script.
    pub async fn eval<T>(&self, source: impl Into<Vec<u8>>) -> Result<T>
    where
        for<'js> T: FromJs<'js> + Send,
    {
        let source = source.into();
        self.context
            .with(move |context| {
                context
                    .eval(source)
                    .catch(&context)
                    .map_err(|error| {
                        eprintln!("JavaScript evaluation failed: {error}");
                        error
                    })
                    .throw(&context)
            })
            .await
    }

    /// Enqueue native window events through [`crate::plugins::WindowEventsPlugin`].
    pub async fn enqueue_window_events(&self, events: &[WindowEventMessage]) -> Result<()> {
        self.context
            .with(move |context| plugins::enqueue(&context, events))
            .await
    }

    /// Evaluate a JavaScript expression that returns a promise.
    ///
    /// Promise reactions are microtasks owned and executed by QuickJS itself.
    pub async fn eval_promise<T>(&self, source: impl Into<Vec<u8>>) -> Result<T>
    where
        for<'js> T: FromJs<'js> + Send + 'static,
    {
        let source = source.into();
        rquickjs::async_with!(self.context => |context| {
            let promise: Promise = context.eval(source)?;
            promise.into_future::<T>().await
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
    use crate::{InputState, MacrotaskQueue, ModifiersState, WindowEventMessage};
    use rquickjs::{prelude::Func, Ctx};

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
    async fn installs_custom_plugins() {
        fn install_answer(context: &Ctx<'_>) -> crate::Result<()> {
            context.globals().set("answer", Func::from(|| 42))?;

            let queue = MacrotaskQueue::from_context(context)?;
            queue.enqueue(|context| {
                context.eval::<(), _>(
                    "globalThis.__order = ['macrotask-1']; \
                     Promise.resolve().then(() => __order.push('microtask'))",
                )
            })?;
            queue.enqueue(|context| context.eval::<(), _>("__order.push('macrotask-2')"))
        }

        let runtime = Runtime::builder()
            .plugin(install_answer)
            .build()
            .await
            .unwrap();

        let value: i32 = runtime.eval("answer()").await.unwrap();
        assert_eq!(value, 42);

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let order: Vec<String> = runtime.eval("__order").await.unwrap();
        assert_eq!(order, ["macrotask-1", "microtask", "macrotask-2"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bare_runtime_keeps_its_event_loop_but_has_no_plugins() {
        let runtime = crate::RuntimeBuilder::bare().build().await.unwrap();

        let globals: Vec<bool> = runtime
            .eval("[typeof console !== 'undefined', typeof setTimeout !== 'undefined']")
            .await
            .unwrap();
        assert_eq!(globals, [false, false]);
        let value: i32 = runtime
            .eval_promise("Promise.resolve().then(() => 42)")
            .await
            .unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatches_native_events_as_macrotasks() {
        let runtime = Runtime::new().await.unwrap();
        runtime
            .eval::<()>(
                r#"
                globalThis.__events = [];
                globalThis.__burokku_dispatch_event = event => __events.push(event.type);
                "#,
            )
            .await
            .unwrap();

        runtime
            .enqueue_window_events(&[
                WindowEventMessage::Resized {
                    width: 800,
                    height: 600,
                },
                WindowEventMessage::CloseRequested,
            ])
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let events: Vec<String> = runtime.eval("__events").await.unwrap();
        assert_eq!(events, ["resized", "close-requested"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn includes_native_input_event_data() {
        let runtime = Runtime::new().await.unwrap();
        runtime
            .eval::<()>(
                r#"
                globalThis.__events = [];
                globalThis.__burokku_dispatch_event = event => __events.push(event);
                "#,
            )
            .await
            .unwrap();

        runtime
            .enqueue_window_events(&[
                WindowEventMessage::CursorMoved { x: 12.5, y: 24.0 },
                WindowEventMessage::KeyboardInput {
                    key_code: 0,
                    text: Some("a".into()),
                    state: InputState::Pressed,
                    repeat: false,
                    modifiers: ModifiersState {
                        shift: true,
                        ..ModifiersState::default()
                    },
                },
            ])
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let json: String = runtime.eval("JSON.stringify(__events)").await.unwrap();
        assert_eq!(
            json,
            r#"[{"type":"cursor-moved","x":12.5,"y":24},{"type":"keyboard-input","keyCode":0,"text":"a","pressed":true,"repeat":false,"shiftKey":true,"ctrlKey":false,"altKey":false,"metaKey":false,"capsLock":false}]"#
        );
    }
}
