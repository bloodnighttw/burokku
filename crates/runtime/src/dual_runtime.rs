//! Composition of a UI-thread isolate and a background JavaScript isolate.

use crate::{Plugin, Result, Runtime, RuntimeBuilder, RuntimeDriver, RuntimeRole};
use std::thread::JoinHandle;
use tokio::sync::oneshot;

const DEFAULT_BACKGROUND_THREAD_NAME: &str = "burokku-js-background";

/// Configures two isolated JavaScript runtimes with explicit responsibilities.
pub struct DualRuntimeBuilder {
    main: RuntimeBuilder,
    background: RuntimeBuilder,
    background_thread_name: String,
}

impl DualRuntimeBuilder {
    pub fn new() -> Self {
        Self {
            main: RuntimeBuilder::new().role(RuntimeRole::Main),
            background: RuntimeBuilder::new().role(RuntimeRole::Background),
            background_thread_name: DEFAULT_BACKGROUND_THREAD_NAME.into(),
        }
    }

    /// Install a plugin only in the latency-sensitive main isolate.
    pub fn main_plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin,
    {
        self.main = self.main.plugin(plugin);
        self
    }

    /// Install a plugin only in the background application isolate.
    pub fn background_plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin,
    {
        self.background = self.background.plugin(plugin);
        self
    }

    /// Customize the dedicated background JavaScript thread's diagnostic name.
    pub fn background_thread_name(mut self, name: impl Into<String>) -> Self {
        self.background_thread_name = name.into();
        self
    }

    /// Configure bounded macrotask capacity for the main isolate.
    pub fn main_macrotask_capacity(mut self, capacity: usize) -> Self {
        self.main = self.main.macrotask_capacity(capacity);
        self
    }

    /// Configure bounded macrotask capacity for the background isolate.
    pub fn background_macrotask_capacity(mut self, capacity: usize) -> Self {
        self.background = self.background.macrotask_capacity(capacity);
        self
    }

    /// Build both isolates and start the background driver on a dedicated thread.
    ///
    /// The returned [`DualRuntimeDriver`] must be polled on the UI/main thread.
    pub async fn build(self) -> Result<(DualRuntime, DualRuntimeDriver)> {
        let (main, main_driver) = self.main.build_driven().await?;
        let (ready_sender, ready_receiver) = oneshot::channel();
        let thread_name = self.background_thread_name;
        let background = self.background;

        let background_thread = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let executor = match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(executor) => executor,
                    Err(_) => {
                        let _ = ready_sender.send(Err(rquickjs::Error::Unknown));
                        return;
                    }
                };

                executor.block_on(async move {
                    match background.build_driven().await {
                        Ok((runtime, driver)) => {
                            if ready_sender.send(Ok(runtime)).is_ok() {
                                driver.run().await;
                            }
                        }
                        Err(error) => {
                            let _ = ready_sender.send(Err(error));
                        }
                    }
                });
            })
            .map_err(|_| rquickjs::Error::Unknown)?;

        let background = ready_receiver
            .await
            .map_err(|_| rquickjs::Error::Unknown)??;

        Ok((
            DualRuntime {
                main,
                background,
                background_thread: Some(background_thread),
            },
            DualRuntimeDriver {
                main: main_driver,
                _thread_affinity: std::marker::PhantomData,
            },
        ))
    }
}

impl Default for DualRuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DualRuntimeBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DualRuntimeBuilder")
            .field("main", &self.main)
            .field("background", &self.background)
            .field("background_thread_name", &self.background_thread_name)
            .finish()
    }
}

/// Two isolated JavaScript runtimes with explicitly separated plugin sets.
pub struct DualRuntime {
    main: Runtime,
    background: Runtime,
    background_thread: Option<JoinHandle<()>>,
}

impl DualRuntime {
    pub fn builder() -> DualRuntimeBuilder {
        DualRuntimeBuilder::new()
    }

    pub fn main(&self) -> &Runtime {
        &self.main
    }

    pub fn background(&self) -> &Runtime {
        &self.background
    }

    /// Shut down both isolates and join the dedicated background thread.
    ///
    /// The main driver must still be polled while this future runs.
    pub async fn shutdown(mut self) -> Result<()> {
        let (main_result, background_result) =
            tokio::join!(self.main.shutdown(), self.background.shutdown());
        main_result?;
        background_result?;

        if let Some(thread) = self.background_thread.take() {
            tokio::task::spawn_blocking(move || thread.join())
                .await
                .map_err(|_| rquickjs::Error::Unknown)?
                .map_err(|_| rquickjs::Error::Unknown)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for DualRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DualRuntime")
            .field("main", &self.main)
            .field("background", &self.background)
            .finish_non_exhaustive()
    }
}

/// The main-isolate half of a [`DualRuntime`]'s execution machinery.
pub struct DualRuntimeDriver {
    main: RuntimeDriver,
    // Native UI loops are thread-affine. Prevent callers from moving the main
    // driver into a general-purpose Tokio worker after construction.
    _thread_affinity: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl DualRuntimeDriver {
    /// Drive the main JavaScript isolate on the caller's current thread.
    pub async fn run(self) {
        self.main.run().await;
    }
}

impl std::fmt::Debug for DualRuntimeDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DualRuntimeDriver")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{prelude::Func, Ctx};

    #[derive(Debug)]
    struct ThreadNamePlugin;

    impl Plugin for ThreadNamePlugin {
        fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
            context.globals().set(
                "runtimeThread",
                Func::from(|| {
                    std::thread::current()
                        .name()
                        .unwrap_or("unnamed")
                        .to_owned()
                }),
            )
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn isolates_plugins_and_executes_background_js_on_its_thread() {
        let (runtime, driver) = DualRuntime::builder()
            .main_plugin(|context: &Ctx<'_>| context.globals().set("mainOnly", true))
            .main_plugin(ThreadNamePlugin)
            .background_plugin(|context: &Ctx<'_>| context.globals().set("backgroundOnly", true))
            .background_plugin(ThreadNamePlugin)
            .build()
            .await
            .unwrap();

        let exercise = async move {
            let main: Vec<bool> = runtime
                .main()
                .eval("[mainOnly, typeof backgroundOnly === 'undefined']")
                .await
                .unwrap();
            let background: Vec<bool> = runtime
                .background()
                .eval("[backgroundOnly, typeof mainOnly === 'undefined']")
                .await
                .unwrap();
            let main_thread: String = runtime.main().eval("runtimeThread()").await.unwrap();
            let background_thread: String =
                runtime.background().eval("runtimeThread()").await.unwrap();

            assert_eq!(main, [true, true]);
            assert_eq!(background, [true, true]);
            assert_ne!(main_thread, background_thread);
            assert_eq!(background_thread, DEFAULT_BACKGROUND_THREAD_NAME);

            runtime.shutdown().await.unwrap();
        };

        tokio::join!(driver.run(), exercise);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn background_runtime_supports_tokio_io() {
        let (io_sender, io_receiver) = std::sync::mpsc::sync_channel(1);
        let (runtime, driver) = DualRuntime::builder()
            .background_plugin(move |_: &Ctx<'_>| {
                let io_sender = io_sender.clone();
                tokio::spawn(async move {
                    let streams = tokio::net::UnixStream::pair();
                    let _ = io_sender.send(streams.is_ok());
                });
                Ok(())
            })
            .build()
            .await
            .unwrap();

        let exercise = async move {
            assert_eq!(
                io_receiver.recv_timeout(std::time::Duration::from_secs(5)),
                Ok(true)
            );
            runtime.shutdown().await.unwrap();
        };

        tokio::join!(driver.run(), exercise);
    }
}
