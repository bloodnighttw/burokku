//! High-level Burokku application construction and lifecycle.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use runtime::{DualRuntime, DualRuntimeBuilder, Plugin};
use thiserror::Error;
use tokio::sync::oneshot;
use winit::{EventLoop, Window};

use crate::ui::{
    elements::SharedDom,
    events::EventDispatcher,
    frame::{FrameError, FrameRenderer, UiApplication},
    js_bridge::DomPlugin,
};

/// Errors produced while constructing or running a Burokku application.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Runtime(#[from] runtime::Error),
    #[error(transparent)]
    Native(#[from] winit::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
}

/// Result type returned by the high-level Burokku API.
pub type Result<T> = std::result::Result<T, Error>;

/// Selects whether an application opens a native window or only evaluates its DOM.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RunMode {
    /// Create a native window and run layout, rendering, and input dispatch.
    #[default]
    Windowed,
    /// Evaluate both scripts without creating native or GPU resources.
    Headless,
}

/// A configured Burokku application.
///
/// Construct applications with [`Burokku::builder`], then call [`Burokku::run`]
/// on the process main thread from a current-thread Tokio runtime.
pub struct Burokku {
    title: String,
    main_script: Vec<u8>,
    background_script: Vec<u8>,
    mode: RunMode,
    runtime: DualRuntimeBuilder,
}

impl Burokku {
    /// Start building an application with an empty main and background script.
    pub fn builder() -> BurokkuBuilder {
        BurokkuBuilder::new()
    }

    /// Run the application until script evaluation fails or the final window closes.
    pub async fn run(self) -> Result<()> {
        match self.mode {
            RunMode::Windowed => self.run_windowed().await,
            RunMode::Headless => self.run_headless().await,
        }
    }

    async fn run_headless(self) -> Result<()> {
        let (dom_plugin, _shared_dom) = DomPlugin::with_new_dom();
        let (runtime, main_driver) = self.runtime.background_plugin(dom_plugin).build().await?;
        let main_script = self.main_script;
        let background_script = self.background_script;

        let lifecycle = async move {
            let evaluation = evaluate_scripts(&runtime, main_script, background_script).await;
            let shutdown = runtime.shutdown().await;
            evaluation?;
            shutdown.map_err(Error::from)
        };

        let ((), result) = tokio::join!(main_driver.run(), lifecycle);
        result
    }

    async fn run_windowed(self) -> Result<()> {
        // Native window and GPU resources are created on MTS before either
        // JavaScript isolate can publish a DOM commit.
        let mut event_loop = EventLoop::new()?;
        let window = Arc::new(
            event_loop.create_window(Window::default_attributes().with_title(self.title))?,
        );
        let renderer = FrameRenderer::new(window.clone()).await?;

        // A BTS publication wakes the demand-driven native loop immediately.
        let event_loop_proxy = event_loop.create_proxy();
        let shared_dom = SharedDom::with_commit_waker(move || event_loop_proxy.wake_up());
        let dom_plugin = DomPlugin::new(shared_dom.clone());
        let (runtime, main_driver) = self.runtime.background_plugin(dom_plugin).build().await?;

        let event_dispatcher = EventDispatcher::new(runtime.background().macrotask_queue());
        let (close_sender, close_receiver) = oneshot::channel();
        let external_exit = Arc::new(AtomicBool::new(false));
        let application = UiApplication::new(
            window,
            shared_dom,
            renderer,
            event_dispatcher,
            close_sender,
            external_exit.clone(),
        );
        let main_script = self.main_script;
        let background_script = self.background_script;

        let runtime_lifecycle = async move {
            let evaluation = evaluate_scripts(&runtime, main_script, background_script).await;
            if evaluation.is_err() {
                external_exit.store(true, Ordering::Release);
            } else {
                // Keep both isolates alive while native events can still target BTS.
                let _ = close_receiver.await;
            }

            let shutdown = runtime.shutdown().await;
            evaluation?;
            shutdown.map_err(Error::from)
        };

        // The native loop and MTS QuickJS driver are both intentionally pinned
        // to this current thread. DualRuntime owns the dedicated BTS thread.
        let ((), native_result, runtime_result) = tokio::join!(
            main_driver.run(),
            event_loop.run_app(application),
            runtime_lifecycle,
        );

        let mut application = native_result?;
        runtime_result?;
        #[cfg(debug_assertions)]
        if std::env::var_os("BUROKKU_PRINT_METRICS").is_some() {
            eprintln!("Burokku performance metrics: {:#?}", application.metrics());
        }
        if let Some(error) = application.take_error() {
            return Err(error.into());
        }
        Ok(())
    }
}

impl std::fmt::Debug for Burokku {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Burokku")
            .field("title", &self.title)
            .field("main_script_bytes", &self.main_script.len())
            .field("background_script_bytes", &self.background_script.len())
            .field("mode", &self.mode)
            .field("runtime", &self.runtime)
            .finish()
    }
}

async fn evaluate_scripts(
    runtime: &DualRuntime,
    main_script: Vec<u8>,
    background_script: Vec<u8>,
) -> Result<()> {
    let (main_result, background_result) = tokio::join!(
        runtime.main().eval::<()>(main_script),
        runtime.background().eval::<()>(background_script),
    );
    main_result?;
    background_result?;
    Ok(())
}

/// Builder for the high-level Burokku application API.
pub struct BurokkuBuilder {
    title: String,
    main_script: Vec<u8>,
    background_script: Vec<u8>,
    mode: RunMode,
    runtime: DualRuntimeBuilder,
}

impl BurokkuBuilder {
    /// Create a builder without optional runtime plugins.
    ///
    /// The Burokku DOM runtime plugin is always added to the background
    /// isolate when the application runs. Console, JSON, timers, and other
    /// capabilities remain explicit.
    pub fn new() -> Self {
        Self {
            title: "Burokku".into(),
            main_script: Vec::new(),
            background_script: Vec::new(),
            mode: RunMode::Windowed,
            runtime: DualRuntime::builder(),
        }
    }

    /// Set the native window title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set JavaScript evaluated in the main-thread isolate.
    pub fn main_script(mut self, source: impl Into<Vec<u8>>) -> Self {
        self.main_script = source.into();
        self
    }

    /// Set application JavaScript evaluated in the background isolate.
    pub fn background_script(mut self, source: impl Into<Vec<u8>>) -> Self {
        self.background_script = source.into();
        self
    }

    /// Replace the underlying dual-runtime configuration.
    ///
    /// Call this before adding plugins through [`runtime_plugin`](Self::runtime_plugin)
    /// or [`main_runtime_plugin`](Self::main_runtime_plugin), because replacement
    /// discards plugins previously added through this builder.
    pub fn dual_runtime(mut self, runtime: DualRuntimeBuilder) -> Self {
        self.runtime = runtime;
        self
    }

    /// Install a plugin in the background application runtime.
    pub fn runtime_plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin,
    {
        self.runtime = self.runtime.background_plugin(plugin);
        self
    }

    /// Install a plugin in the main-thread runtime.
    pub fn main_runtime_plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin,
    {
        self.runtime = self.runtime.main_plugin(plugin);
        self
    }

    /// Select windowed or headless execution explicitly.
    pub fn mode(mut self, mode: RunMode) -> Self {
        self.mode = mode;
        self
    }

    /// Evaluate scripts and DOM commits without native or GPU state.
    pub fn headless(mut self) -> Self {
        self.mode = RunMode::Headless;
        self
    }

    /// Finish configuring the application.
    pub fn build(self) -> Burokku {
        Burokku {
            title: self.title,
            main_script: self.main_script,
            background_script: self.background_script,
            mode: self.mode,
            runtime: self.runtime,
        }
    }

    /// Build and immediately run the application.
    pub async fn run(self) -> Result<()> {
        self.build().run().await
    }
}

impl Default for BurokkuBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for BurokkuBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BurokkuBuilder")
            .field("title", &self.title)
            .field("main_script_bytes", &self.main_script.len())
            .field("background_script_bytes", &self.background_script.len())
            .field("mode", &self.mode)
            .field("runtime", &self.runtime)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{rquickjs::Ctx, RuntimeRole};

    #[tokio::test(flavor = "current_thread")]
    async fn builder_composes_dual_runtime_plugins_and_dom() {
        fn main_plugin(context: &Ctx<'_>) -> runtime::Result<()> {
            assert_eq!(RuntimeRole::from_context(context), Some(RuntimeRole::Main));
            context.globals().set("mainPluginReady", true)
        }

        fn background_plugin(context: &Ctx<'_>) -> runtime::Result<()> {
            assert_eq!(
                RuntimeRole::from_context(context),
                Some(RuntimeRole::Background)
            );
            context.globals().set("backgroundPluginReady", true)
        }

        Burokku::builder()
            .dual_runtime(DualRuntime::builder().background_thread_name("burokku-test-bts"))
            .main_runtime_plugin(main_plugin)
            .runtime_plugin(background_plugin)
            .main_script("if (!mainPluginReady) throw new Error('missing main plugin')")
            .background_script(
                "if (!backgroundPluginReady || !document.body) \
                 throw new Error('missing background plugins')",
            )
            .headless()
            .run()
            .await
            .unwrap();
    }
}
