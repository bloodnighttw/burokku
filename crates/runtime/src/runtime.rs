use crate::{
    event_loop::{self, RuntimeControl},
    JsTaskQueue, Result, RuntimeBuilder,
};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, FromJs, Promise, ThrowResultExt};
use std::{future::Future, marker::PhantomData, pin::Pin, rc::Rc};
use tokio::sync::oneshot;

/// A thread-safe handle to one thread-affine JavaScript runtime.
///
/// All JavaScript entry points are submitted to the isolate's macrotask queue.
/// JavaScript executes only where the associated [`RuntimeDriver`] is polled.
pub struct Runtime {
    macrotasks: JsTaskQueue,
    control: RuntimeControl,
    shutdown_requested: bool,
}

/// Drives the futures, jobs, and macrotasks belonging to one QuickJS isolate.
///
/// The driver is deliberately `!Send`. It must be spawned with
/// [`tokio::task::spawn_local`] and remain on one persistent
/// [`tokio::task::LocalSet`] for its whole lifetime.
pub struct RuntimeDriver {
    context: Option<AsyncContext>,
    quickjs: Pin<Box<dyn Future<Output = ()> + 'static>>,
    stopped: oneshot::Receiver<()>,
    _local: PhantomData<Rc<()>>,
}

impl RuntimeDriver {
    /// Run this isolate until its task queue is shut down.
    pub async fn run(mut self) {
        tokio::select! {
            _ = &mut self.quickjs => {}
            _ = &mut self.stopped => {}
        }

        // The context is deliberately owned by the driver so callers cannot
        // execute QuickJS directly from a different thread.
        self.context.take();
    }
}

impl Runtime {
    /// Start configuring a thread-affine runtime without host plugins.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    pub(crate) async fn build_driven<F>(
        builder: RuntimeBuilder,
        installer: F,
    ) -> Result<(Self, RuntimeDriver)>
    where
        F: for<'js> FnOnce(&rquickjs::Ctx<'js>) -> Result<()> + 'static,
    {
        let quickjs = AsyncRuntime::new()?;
        let context = AsyncContext::full(&quickjs).await?;
        let macrotask_capacity = builder.macrotask_capacity;
        let plugins = builder.plugins;

        context
            .with(move |context| {
                // Host globals are opt-in plugins. Keep QuickJS's underlying
                // JSON support, but expose its public global through JsonPlugin.
                context.globals().remove("JSON")
            })
            .await?;

        let (macrotasks, control, stopped) =
            event_loop::install(&context, macrotask_capacity, plugins).await?;
        context.with(move |context| installer(&context)).await?;

        let driver = RuntimeDriver {
            context: Some(context),
            quickjs: Box::pin(quickjs.drive()),
            stopped,
            _local: PhantomData,
        };
        let runtime = Self {
            macrotasks,
            control,
            shutdown_requested: false,
        };

        Ok((runtime, driver))
    }

    /// Clone a handle that can enqueue native macrotasks in this runtime.
    pub fn macrotask_queue(&self) -> JsTaskQueue {
        self.macrotasks.clone()
    }

    /// Evaluate a synchronous JavaScript expression or script as a macrotask.
    pub async fn eval<T>(&self, source: impl Into<Vec<u8>>) -> Result<T>
    where
        for<'js> T: FromJs<'js> + Send + 'static,
    {
        let source = source.into();
        let (sender, receiver) = oneshot::channel();

        self.macrotasks
            .enqueue(move |context| {
                let result = context
                    .eval(source)
                    .catch(context)
                    .map_err(|error| {
                        eprintln!("JavaScript evaluation failed: {error}");
                        error
                    })
                    .throw(context);
                let _ = sender.send(result);
                Ok(())
            })
            .await
            .map_err(|_| rquickjs::Error::Unknown)?;

        receiver.await.map_err(|_| rquickjs::Error::Unknown)?
    }

    /// Evaluate JavaScript that returns a promise as a macrotask.
    pub async fn eval_promise<T>(&self, source: impl Into<Vec<u8>>) -> Result<T>
    where
        for<'js> T: FromJs<'js> + Send + 'static,
    {
        let source = source.into();
        let (sender, receiver) = oneshot::channel();

        self.macrotasks
            .enqueue(move |context| {
                let promise: Promise = match context.eval(source) {
                    Ok(promise) => promise,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return Ok(());
                    }
                };
                context.spawn(async move {
                    let _ = sender.send(promise.into_future::<T>().await);
                });
                Ok(())
            })
            .await
            .map_err(|_| rquickjs::Error::Unknown)?;

        receiver.await.map_err(|_| rquickjs::Error::Unknown)?
    }

    /// Stop accepting tasks and wait until the local driver observes shutdown.
    pub async fn shutdown(mut self) -> Result<()> {
        self.shutdown_requested = true;
        let stopped = self
            .control
            .request_shutdown()
            .map_err(|_| rquickjs::Error::Unknown)?;
        stopped.await.map_err(|_| rquickjs::Error::Unknown)?;
        Ok(())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if !self.shutdown_requested {
            self.control.request_shutdown_without_waiting();
        }
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Runtime").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RuntimeDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeDriver")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::Runtime;
    use crate::{JsTaskQueue, JsTaskQueueError};
    use rquickjs::{prelude::Func, Ctx};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tokio::task::LocalSet;

    #[tokio::test(flavor = "current_thread")]
    async fn evaluates_javascript_and_promises() {
        LocalSet::new()
            .run_until(async {
                let (runtime, driver) = Runtime::builder().build_driven().await.unwrap();
                let driver = tokio::task::spawn_local(driver.run());

                assert_eq!(runtime.eval::<i32>("20 + 22").await.unwrap(), 42);
                assert_eq!(
                    runtime
                        .eval_promise::<String>("Promise.resolve('Burokku')")
                        .await
                        .unwrap(),
                    "Burokku"
                );

                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn installs_local_plugins_and_preserves_task_order() {
        LocalSet::new()
            .run_until(async {
                fn install_answer(context: &Ctx<'_>) -> crate::Result<()> {
                    context.globals().set("answer", Func::from(|| 42))?;
                    let queue = JsTaskQueue::from_context(context)?;
                    queue
                        .try_enqueue(|context| {
                            context.eval::<(), _>(
                                "globalThis.__order = ['macrotask-1']; \
                                 Promise.resolve().then(() => __order.push('microtask'))",
                            )
                        })
                        .map_err(|_| rquickjs::Error::Unknown)?;
                    queue
                        .try_enqueue(|context| context.eval::<(), _>("__order.push('macrotask-2')"))
                        .map_err(|_| rquickjs::Error::Unknown)
                }

                let (runtime, driver) = Runtime::builder()
                    .plugin(install_answer)
                    .build_driven()
                    .await
                    .unwrap();
                let driver = tokio::task::spawn_local(driver.run());

                assert_eq!(runtime.eval::<i32>("answer()").await.unwrap(), 42);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                let order: Vec<String> = runtime.eval("__order").await.unwrap();
                assert_eq!(order, ["macrotask-1", "microtask", "macrotask-2"]);

                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_has_no_implicit_host_plugins() {
        LocalSet::new()
            .run_until(async {
                let (runtime, driver) = Runtime::builder().build_driven().await.unwrap();
                let driver = tokio::task::spawn_local(driver.run());
                let globals: Vec<bool> = runtime
                    .eval(
                        "[typeof console !== 'undefined', \
                         typeof setTimeout !== 'undefined', \
                         typeof JSON !== 'undefined']",
                    )
                    .await
                    .unwrap();
                assert_eq!(globals, [false, false, false]);
                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bounded_queue_reports_backpressure() {
        LocalSet::new()
            .run_until(async {
                let (runtime, driver) = Runtime::builder()
                    .macrotask_capacity(1)
                    .build_driven()
                    .await
                    .unwrap();
                let queue = runtime.macrotask_queue();
                assert_eq!(queue.max_capacity(), 1);
                queue.try_enqueue(|_| Ok(())).unwrap();
                assert_eq!(queue.try_enqueue(|_| Ok(())), Err(JsTaskQueueError::Full));

                let driver = tokio::task::spawn_local(driver.run());
                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_bypasses_queued_macrotasks() {
        LocalSet::new()
            .run_until(async {
                let (runtime, driver) = Runtime::builder()
                    .macrotask_capacity(1)
                    .build_driven()
                    .await
                    .unwrap();
                let executed = Arc::new(AtomicBool::new(false));
                runtime
                    .macrotask_queue()
                    .try_enqueue({
                        let executed = Arc::clone(&executed);
                        move |_| {
                            executed.store(true, Ordering::Release);
                            Ok(())
                        }
                    })
                    .unwrap();
                let acknowledged = runtime.control.request_shutdown().unwrap();
                let ((), acknowledged) = tokio::join!(driver.run(), acknowledged);
                acknowledged.unwrap();
                assert!(!executed.load(Ordering::Acquire));
            })
            .await;
    }
}
