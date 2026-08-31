//! Public application builder and end-to-end host assembly.

use std::{cell::RefCell, rc::Rc};

use llrt_utils::primordials::{BasePrimordials, Primordial};
use thiserror::Error;
use tokio::sync::oneshot;

use crate::{
    runtime::{Plugin, RuntimeBuilder},
    ui::{dom_plugin::DomPlugin, host::ApplicationHost, text::TextEngine},
};

fn install_llrt_globals(context: &runtime::rquickjs::Ctx<'_>) -> runtime::Result<()> {
    BasePrimordials::init(context)?;
    let (_, _, globals) = llrt_modules::module_builder::ModuleBuilder::default().build();
    globals.attach(context)?;
    context.eval::<(), _>(include_str!("ui/scripts/llrt_lifecycle.js"))
}

async fn prepare_llrt_shutdown(runtime: &runtime::Runtime) -> runtime::Result<()> {
    runtime.eval::<()>("globalThis.__burokkuShutdown()").await?;
    // LLRT timer callbacks are persistent QuickJS values. Give its notified
    // timer task a chance to release them before the context is dropped.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeStatus {
    Starting,
    Running,
    Failed(String),
    Stopped,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeLifecycle {
    status: Rc<RefCell<RuntimeStatus>>,
    shutdown: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    proxy: Option<winit::EventLoopProxy>,
}

impl RuntimeLifecycle {
    pub(crate) fn new(proxy: winit::EventLoopProxy) -> (Self, oneshot::Receiver<()>) {
        let (shutdown, requested) = oneshot::channel();
        (
            Self {
                status: Rc::new(RefCell::new(RuntimeStatus::Starting)),
                shutdown: Rc::new(RefCell::new(Some(shutdown))),
                proxy: Some(proxy),
            },
            requested,
        )
    }

    pub(crate) fn status(&self) -> RuntimeStatus {
        self.status.borrow().clone()
    }

    fn set_status(&self, status: RuntimeStatus) {
        *self.status.borrow_mut() = status;
        if let Some(proxy) = &self.proxy {
            proxy.wake_up();
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        let (shutdown, _requested) = oneshot::channel();
        Self {
            status: Rc::new(RefCell::new(RuntimeStatus::Starting)),
            shutdown: Rc::new(RefCell::new(Some(shutdown))),
            proxy: None,
        }
    }

    pub(crate) fn request_shutdown(&self) {
        if let Some(shutdown) = self.shutdown.borrow_mut().take() {
            let _ = shutdown.send(());
        }
    }
}

async fn bootstrap_runtime(
    builder: RuntimeBuilder,
    script: Vec<u8>,
    lifecycle: RuntimeLifecycle,
    mut shutdown: oneshot::Receiver<()>,
) {
    let (runtime, driver) = match builder.build_driven().await {
        Ok(runtime) => runtime,
        Err(error) => {
            lifecycle.set_status(RuntimeStatus::Failed(format!(
                "failed to build QuickJS runtime: {error}"
            )));
            return;
        }
    };
    let mut driver = tokio::task::spawn_local(driver.run());

    if let Err(error) = runtime.eval::<()>(script).await {
        let _ = prepare_llrt_shutdown(&runtime).await;
        let _ = runtime.shutdown().await;
        let _ = driver.await;
        lifecycle.set_status(RuntimeStatus::Failed(format!(
            "application script failed: {error}"
        )));
        return;
    }
    lifecycle.set_status(RuntimeStatus::Running);

    tokio::select! {
        _ = &mut shutdown => {
            let cleanup_error = prepare_llrt_shutdown(&runtime).await.err();
            if let Err(error) = runtime.shutdown().await {
                lifecycle.set_status(RuntimeStatus::Failed(format!(
                    "QuickJS shutdown failed: {error}"
                )));
                return;
            }
            if let Err(error) = driver.await {
                lifecycle.set_status(RuntimeStatus::Failed(format!(
                    "QuickJS driver join failed: {error}"
                )));
                return;
            }
            if let Some(error) = cleanup_error {
                lifecycle.set_status(RuntimeStatus::Failed(format!(
                    "LLRT cleanup failed: {error}"
                )));
            } else {
                lifecycle.set_status(RuntimeStatus::Stopped);
            }
        }
        result = &mut driver => {
            let message = match result {
                Ok(()) => "QuickJS driver stopped unexpectedly".to_owned(),
                Err(error) => format!("QuickJS driver failed: {error}"),
            };
            lifecycle.set_status(RuntimeStatus::Failed(message));
        }
    }
}

/// A configured Burokku application.
pub struct Burokku {
    runtime: RuntimeBuilder,
    script: Vec<u8>,
    fonts: Vec<Vec<u8>>,
}

impl Burokku {
    pub fn builder() -> BurokkuBuilder {
        BurokkuBuilder::new()
    }

    /// Run the UI-thread JavaScript runtime and native application lifecycle.
    ///
    /// This synchronous entry point must be called on the process main thread.
    pub fn run(self) -> Result<(), BurokkuError> {
        let mut event_loop = winit::EventLoop::new()?;
        let proxy = event_loop.create_proxy();
        let local_set = tokio::task::LocalSet::new();

        let (dom_plugin, dom) = DomPlugin::new();
        let (lifecycle, shutdown) = RuntimeLifecycle::new(proxy);

        let mut text = TextEngine::new();
        for font in self.fonts {
            text.register_font_data(font)
                .map_err(|error| BurokkuError::Host(error.to_string()))?;
        }

        let host = ApplicationHost::new(dom, text, lifecycle.clone());
        local_set.spawn_local(bootstrap_runtime(
            self.runtime.plugin(dom_plugin),
            self.script,
            lifecycle.clone(),
            shutdown,
        ));

        let host = event_loop.run_app_external(host, local_set)?;
        if let Some(error) = host.fatal_error() {
            return Err(BurokkuError::Host(error.to_string()));
        }
        if let RuntimeStatus::Failed(message) = lifecycle.status() {
            return Err(BurokkuError::Host(message));
        }
        Ok(())
    }
}

/// Builder for JavaScript source, plugins, and embedded fonts.
pub struct BurokkuBuilder {
    runtime: RuntimeBuilder,
    script: Vec<u8>,
    fonts: Vec<Vec<u8>>,
}

impl BurokkuBuilder {
    pub fn new() -> Self {
        let runtime = RuntimeBuilder::new().plugin(install_llrt_globals);
        Self {
            runtime,
            script: Vec::new(),
            fonts: Vec::new(),
        }
    }

    /// Set the bundled JavaScript application source.
    pub fn script(mut self, source: impl Into<Vec<u8>>) -> Self {
        self.script = source.into();
        self
    }

    /// Install an application plugin in the JavaScript runtime.
    pub fn runtime_plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.runtime = self.runtime.plugin(plugin);
        self
    }

    /// Register an embedded OpenType font before the first layout frame.
    pub fn font_data(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.fonts.push(data.into());
        self
    }

    pub fn build(self) -> Burokku {
        Burokku {
            runtime: self.runtime,
            script: self.script,
            fonts: self.fonts,
        }
    }

    pub fn run(self) -> Result<(), BurokkuError> {
        self.build().run()
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
            .field("runtime", &self.runtime)
            .field("script_bytes", &self.script.len())
            .field("fonts", &self.fonts.len())
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum BurokkuError {
    #[error(transparent)]
    Window(#[from] winit::Error),

    #[error("failed to build the UI Tokio runtime: {0}")]
    Tokio(#[from] std::io::Error),

    #[error(transparent)]
    JavaScript(#[from] runtime::Error),

    #[error("the JavaScript runtime stopped before the native event loop")]
    MainRuntimeStopped,

    #[error("application host failed: {0}")]
    Host(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        runtime::Runtime,
        ui::{
            layout::{LayoutEngine, LogicalViewport},
            scene::{PaintItem, ScenePlan},
            window_host::WindowSpec,
        },
    };
    use tokio::task::LocalSet;

    #[test]
    fn builder_collects_script_fonts_and_runtime_configuration() {
        let app = Burokku::builder()
            .script("app.appendChild(app.createElement('window'));".as_bytes())
            .font_data(vec![1, 2, 3])
            .build();
        assert!(!app.script.is_empty());
        assert_eq!(app.fonts, [vec![1, 2, 3]]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn app_script_commits_a_window_and_text_for_the_native_host() {
        LocalSet::new()
            .run_until(async {
                let (plugin, dom) = DomPlugin::new();
                let initial_revision = dom.borrow().dom.revision();
                let (runtime, driver) = Runtime::builder()
                    .plugin(install_llrt_globals)
                    .plugin(plugin)
                    .build_driven()
                    .await
                    .unwrap();
                let driver = tokio::task::spawn_local(driver.run());

                runtime
                    .eval::<()>(
                        "const win = app.createElement('window');\n\
                         win.setAttribute('title', 'Script host');\n\
                         const text = app.createElement('text');\n\
                         text.style.setProperty('font-family', 'Noto Sans');\n\
                         text.textContent = 'Visible';\n\
                         win.appendChild(text);\n\
                         app.appendChild(win);",
                    )
                    .await
                    .unwrap();

                let mut text = TextEngine::without_system_fonts();
                text.register_font_data(
                    include_bytes!("../testdata/fonts/NotoSans-Regular.ttf").to_vec(),
                )
                .unwrap();
                let mut layout = LayoutEngine::new(text);
                {
                    let state = dom.borrow();
                    assert!(state.dom.revision() > initial_revision);
                    let spec = WindowSpec::from_dom(&state.dom).unwrap().unwrap();
                    assert_eq!(spec.title(), "Script host");
                    assert_eq!(state.dom.children(spec.dom_id()).unwrap().len(), 1);
                    let computed = layout
                        .compute(&state.dom, LogicalViewport::new(800.0, 600.0).unwrap())
                        .unwrap();
                    let plan = ScenePlan::from_layout(
                        &state.dom,
                        computed,
                        winit::PhysicalSize::new(800, 600),
                        1.0,
                    )
                    .unwrap();
                    assert!(plan
                        .items()
                        .iter()
                        .any(|item| matches!(item, PaintItem::Text { .. })));
                }
                prepare_llrt_shutdown(&runtime).await.unwrap();
                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn detached_window_may_be_mounted_by_a_later_timer_batch() {
        LocalSet::new()
            .run_until(async {
                let (plugin, dom) = DomPlugin::new();
                let (runtime, driver) = Runtime::builder()
                    .plugin(install_llrt_globals)
                    .plugin(plugin)
                    .build_driven()
                    .await
                    .unwrap();
                let driver = tokio::task::spawn_local(driver.run());

                runtime
                    .eval::<()>(
                        "globalThis.pendingWindow = app.createElement('window');\n\
                         setTimeout(() => app.appendChild(pendingWindow), 10);",
                    )
                    .await
                    .unwrap();
                assert_eq!(WindowSpec::from_dom(&dom.borrow().dom).unwrap(), None);

                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                assert!(WindowSpec::from_dom(&dom.borrow().dom).unwrap().is_some());

                prepare_llrt_shutdown(&runtime).await.unwrap();
                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn llrt_counter_example_updates_from_an_interval() {
        LocalSet::new()
            .run_until(async {
                let (plugin, dom) = DomPlugin::new();
                let (runtime, driver) = Runtime::builder()
                    .plugin(install_llrt_globals)
                    .plugin(plugin)
                    .build_driven()
                    .await
                    .unwrap();
                let driver = tokio::task::spawn_local(driver.run());
                let script = format!(
                    "globalThis.__BUROKKU_COUNTER_INTERVAL_MS__ = 5;\n{}",
                    include_str!("../../../example/counter/src/app.js")
                );

                runtime.eval::<()>(script).await.unwrap();
                assert_eq!(
                    WindowSpec::from_dom(&dom.borrow().dom)
                        .unwrap()
                        .unwrap()
                        .title(),
                    "LLRT counter — 0"
                );
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                assert_ne!(
                    WindowSpec::from_dom(&dom.borrow().dom)
                        .unwrap()
                        .unwrap()
                        .title(),
                    "LLRT counter — 0"
                );

                prepare_llrt_shutdown(&runtime).await.unwrap();
                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }
}
