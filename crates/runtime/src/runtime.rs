use crate::{
    event_loop::{self, RuntimeControl},
    MacrotaskQueue, Result, RuntimeBuilder, RuntimeRole,
};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, FromJs, Promise, ThrowResultExt};
use std::{future::Future, pin::Pin};
use tokio::{sync::oneshot, task::JoinHandle};

/// A thread-safe handle to one isolated JavaScript runtime.
///
/// All JavaScript entry points are submitted to the isolate's macrotask queue.
/// Consequently, JavaScript executes wherever the associated
/// [`RuntimeDriver`] is polled rather than on the caller's thread.
pub struct Runtime {
    role: RuntimeRole,
    macrotasks: MacrotaskQueue,
    control: RuntimeControl,
    spawned_driver: Option<JoinHandle<()>>,
    shutdown_requested: bool,
}

/// Drives the futures, jobs, and macrotasks belonging to one QuickJS isolate.
///
/// A driver should be polled on exactly one assigned thread for its entire
/// lifetime. Dropping the owning [`Runtime`], or explicitly shutting it down,
/// completes the driver.
pub struct RuntimeDriver {
    context: Option<AsyncContext>,
    quickjs: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    stopped: oneshot::Receiver<()>,
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
    /// Start configuring a standalone runtime without host plugins.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }

    /// Create and automatically drive a standalone runtime without plugins.
    ///
    /// This must be called from inside a running Tokio runtime. Use
    /// [`RuntimeBuilder::build_driven`] when the isolate must remain pinned to
    /// a particular thread.
    pub async fn new() -> Result<Self> {
        Self::builder().build().await
    }

    /// Create an automatically driven runtime and install one small host.
    pub async fn new_with_host<F>(installer: F) -> Result<Self>
    where
        F: for<'js> FnOnce(&rquickjs::Ctx<'js>) -> Result<()> + Send + 'static,
    {
        Self::build(RuntimeBuilder::new(), installer).await
    }

    pub(crate) async fn build<F>(builder: RuntimeBuilder, installer: F) -> Result<Self>
    where
        F: for<'js> FnOnce(&rquickjs::Ctx<'js>) -> Result<()> + Send + 'static,
    {
        let (mut runtime, driver) = Self::build_driven(builder, installer).await?;
        runtime.spawned_driver = Some(tokio::spawn(driver.run()));
        Ok(runtime)
    }

    pub(crate) async fn build_driven<F>(
        builder: RuntimeBuilder,
        installer: F,
    ) -> Result<(Self, RuntimeDriver)>
    where
        F: for<'js> FnOnce(&rquickjs::Ctx<'js>) -> Result<()> + Send + 'static,
    {
        let quickjs = AsyncRuntime::new()?;
        let context = AsyncContext::full(&quickjs).await?;
        let role = builder.role;
        let macrotask_capacity = builder.macrotask_capacity;
        let plugins = builder.plugins;

        context
            .with(move |context| {
                // Host globals are opt-in plugins. Keep QuickJS's underlying
                // JSON support, but expose its public global through JsonPlugin.
                context.globals().remove("JSON")?;
                context
                    .store_userdata(role)
                    .map_err(|_| rquickjs::Error::Unknown)
            })
            .await?;

        let (macrotasks, control, stopped) =
            event_loop::install(&context, macrotask_capacity, plugins).await?;
        context.with(move |context| installer(&context)).await?;

        let driver = RuntimeDriver {
            context: Some(context),
            quickjs: Box::pin(quickjs.drive()),
            stopped,
        };
        let runtime = Self {
            role,
            macrotasks,
            control,
            spawned_driver: None,
            shutdown_requested: false,
        };

        Ok((runtime, driver))
    }

    /// The responsibility assigned to this isolate.
    pub fn role(&self) -> RuntimeRole {
        self.role
    }

    /// Clone a handle that can enqueue native macrotasks in this runtime.
    pub fn macrotask_queue(&self) -> MacrotaskQueue {
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

    /// Stop accepting tasks and wait for an automatically spawned driver.
    pub async fn shutdown(mut self) -> Result<()> {
        self.shutdown_requested = true;
        let stopped = self
            .control
            .request_shutdown()
            .map_err(|_| rquickjs::Error::Unknown)?;
        stopped.await.map_err(|_| rquickjs::Error::Unknown)?;

        if let Some(driver) = self.spawned_driver.take() {
            driver.await.map_err(|_| rquickjs::Error::Unknown)?;
        }
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
        formatter
            .debug_struct("Runtime")
            .field("role", &self.role)
            .finish_non_exhaustive()
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
    use crate::{MacrotaskQueue, MacrotaskQueueError, Plugin, RuntimeRole};
    use rquickjs::{prelude::Func, Ctx};
    use std::sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        Arc, Mutex,
    };

    struct RecordingCheckpoint {
        current_value: Arc<AtomicI32>,
        values: Arc<Mutex<Vec<i32>>>,
    }

    impl Plugin for RecordingCheckpoint {
        fn install<'js>(&self, context: &Ctx<'js>) -> crate::Result<()> {
            let current_value = self.current_value.clone();
            context.globals().set(
                "setCheckpointValue",
                Func::from(move |value| current_value.store(value, Ordering::Release)),
            )
        }

        fn checkpoint(&mut self) -> crate::Result<()> {
            let value = self.current_value.load(Ordering::Acquire);
            self.values.lock().unwrap().push(value);
            Ok(())
        }
    }

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
    async fn runtime_keeps_its_event_loop_but_has_no_implicit_plugins() {
        let runtime = Runtime::new().await.unwrap();

        let globals: Vec<bool> = runtime
            .eval(
                "[typeof console !== 'undefined', \
                 typeof setTimeout !== 'undefined', \
                 typeof JSON !== 'undefined']",
            )
            .await
            .unwrap();
        assert_eq!(globals, [false, false, false]);
        let value: i32 = runtime
            .eval_promise("Promise.resolve().then(() => 42)")
            .await
            .unwrap();

        assert_eq!(value, 42);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn plugins_can_read_the_runtime_role() {
        fn install_role(context: &Ctx<'_>) -> crate::Result<()> {
            let role = RuntimeRole::from_context(context).unwrap();
            context.globals().set("isMain", role == RuntimeRole::Main)
        }

        let runtime = Runtime::builder()
            .role(RuntimeRole::Main)
            .plugin(install_role)
            .build()
            .await
            .unwrap();

        assert!(runtime.eval::<bool>("isMain").await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn plugin_checkpoint_runs_after_microtasks_and_failed_macrotasks() {
        let current_value = Arc::new(AtomicI32::new(0));
        let values = Arc::new(Mutex::new(Vec::new()));
        let runtime = Runtime::builder()
            .plugin(RecordingCheckpoint {
                current_value,
                values: values.clone(),
            })
            .build()
            .await
            .unwrap();

        runtime
            .eval::<()>(
                "setCheckpointValue(1); \
                 Promise.resolve().then(() => setCheckpointValue(2))",
            )
            .await
            .unwrap();
        assert_eq!(*values.lock().unwrap(), [2]);

        assert!(runtime
            .eval::<()>("setCheckpointValue(7); throw new Error('failed')")
            .await
            .is_err());
        assert_eq!(*values.lock().unwrap(), [2, 7]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicitly_driven_runtime_executes_queued_work() {
        let (runtime, driver) = Runtime::builder().build_driven().await.unwrap();
        let driver = tokio::spawn(driver.run());

        assert_eq!(runtime.eval::<i32>("6 * 7").await.unwrap(), 42);
        runtime.shutdown().await.unwrap();
        driver.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bounded_queue_reports_backpressure_to_synchronous_producers() {
        let (runtime, driver) = Runtime::builder()
            .macrotask_capacity(1)
            .build_driven()
            .await
            .unwrap();
        let queue = runtime.macrotask_queue();

        assert_eq!(queue.max_capacity(), 1);
        assert_eq!(queue.depth(), 0);
        queue.try_enqueue(|_| Ok(())).unwrap();
        assert_eq!(queue.capacity(), 0);
        assert_eq!(queue.depth(), 1);
        assert_eq!(
            queue.try_enqueue(|_| Ok(())),
            Err(MacrotaskQueueError::Full)
        );

        let driver = tokio::spawn(driver.run());
        runtime.shutdown().await.unwrap();
        driver.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_bypasses_queued_macrotasks() {
        let (runtime, driver) = Runtime::builder()
            .macrotask_capacity(1)
            .build_driven()
            .await
            .unwrap();
        let executed = Arc::new(AtomicBool::new(false));

        runtime
            .macrotask_queue()
            .try_enqueue({
                let executed = executed.clone();
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
    }
}
