//! Public application builder and end-to-end host assembly.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::sync::Notify;

use crate::{
    runtime::{
        plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
        DualRuntime, DualRuntimeBuilder, Plugin,
    },
    ui::{
        dom_plugin::DomPlugin,
        gpu::{GraphicsContext, WindowRenderer},
        host::ApplicationHost,
        text::TextEngine,
        window_host::{WindowManager, WindowSpec},
    },
};

/// A configured Burokku application.
pub struct Burokku {
    runtime: DualRuntimeBuilder,
    script: Vec<u8>,
    fonts: Vec<Vec<u8>>,
}

impl Burokku {
    pub fn builder() -> BurokkuBuilder {
        BurokkuBuilder::new()
    }

    /// Run JavaScript on BTS and drive the native window until it closes.
    ///
    /// This future must be polled on the process main thread. On macOS, use a
    /// current-thread Tokio runtime so AppKit, the main QuickJS isolate, layout,
    /// and presentation remain thread-affine.
    pub async fn run(self) -> Result<(), BurokkuError> {
        let mut event_loop = winit::EventLoop::new()?;
        let proxy = event_loop.create_proxy();
        let committed = Arc::new(Notify::new());
        let notifier = Arc::clone(&committed);
        let (dom_plugin, publications) = DomPlugin::new(move |_| {
            notifier.notify_one();
            proxy.wake_up();
        });
        let initial_revision = publications.load().revision();
        let (runtime, driver) = self.runtime.background_plugin(dom_plugin).build().await?;
        let driver_future = driver.run();
        tokio::pin!(driver_future);

        if let Err(error) = runtime.background().eval::<()>(self.script).await {
            let _ = shutdown_with_driver(runtime, driver_future.as_mut()).await;
            return Err(BurokkuError::JavaScript(error));
        }

        let initial =
            match wait_for_initial_window(&publications, initial_revision, &committed).await {
                Ok(initial) => initial,
                Err(error) => {
                    let _ = shutdown_with_driver(runtime, driver_future.as_mut()).await;
                    return Err(error);
                }
            };
        let windows = match WindowManager::create_initial(&mut event_loop, &initial) {
            Ok(windows) => windows,
            Err(error) => {
                let _ = shutdown_with_driver(runtime, driver_future.as_mut()).await;
                return Err(BurokkuError::Host(error.to_string()));
            }
        };
        let native_window = Arc::clone(
            windows
                .current()
                .expect("initial WindowManager contains a native window")
                .window(),
        );
        event_loop.flush_windows();
        let graphics = match GraphicsContext::for_window(Arc::clone(&native_window)).await {
            Ok(graphics) => graphics,
            Err(error) => {
                let _ = shutdown_with_driver(runtime, driver_future.as_mut()).await;
                return Err(BurokkuError::Host(error.to_string()));
            }
        };
        let renderer = match WindowRenderer::new(&graphics, native_window) {
            Ok(renderer) => renderer,
            Err(error) => {
                let _ = shutdown_with_driver(runtime, driver_future.as_mut()).await;
                return Err(BurokkuError::Host(error.to_string()));
            }
        };
        let mut text = TextEngine::new();
        for font in self.fonts {
            if let Err(error) = text.register_font_data(font) {
                let _ = shutdown_with_driver(runtime, driver_future.as_mut()).await;
                return Err(BurokkuError::Host(error.to_string()));
            }
        }

        let host = ApplicationHost::new(publications, initial, graphics, windows, renderer, text);
        let event_future = event_loop.run_app(host);
        tokio::pin!(event_future);
        let mut driver_finished = false;
        let event_result = tokio::select! {
            result = &mut event_future => Some(result),
            () = &mut driver_future => {
                driver_finished = true;
                None
            }
        };

        let shutdown_result = if driver_finished {
            runtime.shutdown().await
        } else {
            shutdown_with_driver(runtime, driver_future.as_mut()).await
        };

        let host = match event_result {
            Some(Ok(host)) => host,
            Some(Err(error)) => {
                let _ = shutdown_result;
                return Err(BurokkuError::Window(error));
            }
            None => {
                shutdown_result?;
                return Err(BurokkuError::MainRuntimeStopped);
            }
        };
        if let Some(error) = host.fatal_error() {
            let message = error.to_string();
            let _ = shutdown_result;
            return Err(BurokkuError::Host(message));
        }
        let _last_presented_revision = host.presented().map(|frame| frame.revision());
        shutdown_result?;
        Ok(())
    }
}

/// Builder for JavaScript source, plugins, and embedded fonts.
pub struct BurokkuBuilder {
    runtime: DualRuntimeBuilder,
    script: Vec<u8>,
    fonts: Vec<Vec<u8>>,
}

impl BurokkuBuilder {
    pub fn new() -> Self {
        let runtime = DualRuntimeBuilder::new()
            .main_plugin(ConsolePlugin)
            .main_plugin(JsonPlugin)
            .main_plugin(TimersPlugin)
            .background_plugin(ConsolePlugin)
            .background_plugin(JsonPlugin)
            .background_plugin(TimersPlugin);
        Self {
            runtime,
            script: Vec::new(),
            fonts: Vec::new(),
        }
    }

    /// Set the bundled JavaScript application source evaluated on BTS.
    pub fn script(mut self, source: impl Into<Vec<u8>>) -> Self {
        self.script = source.into();
        self
    }

    /// Install an application plugin in the background JavaScript isolate.
    pub fn runtime_plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.runtime = self.runtime.background_plugin(plugin);
        self
    }

    /// Install a latency-sensitive plugin in the main JavaScript isolate.
    pub fn main_runtime_plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.runtime = self.runtime.main_plugin(plugin);
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

    pub async fn run(self) -> Result<(), BurokkuError> {
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
            .field("runtime", &self.runtime)
            .field("script_bytes", &self.script.len())
            .field("fonts", &self.fonts.len())
            .finish()
    }
}

async fn wait_for_initial_window(
    publications: &crate::ui::elements::PublishedDomReader,
    initial_revision: u64,
    committed: &Notify,
) -> Result<Arc<crate::ui::elements::PublishedDom>, BurokkuError> {
    let wait = async {
        loop {
            let publication = publications.load();
            if publication.revision() > initial_revision {
                return match WindowSpec::from_publication(&publication) {
                    Ok(Some(_)) => Ok(publication),
                    Ok(None) => Err(BurokkuError::MissingWindow),
                    Err(error) => Err(BurokkuError::Host(error.to_string())),
                };
            }
            committed.notified().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .unwrap_or(Err(BurokkuError::MissingWindow))
}

async fn shutdown_with_driver<F>(
    runtime: DualRuntime,
    mut driver: Pin<&mut F>,
) -> runtime::Result<()>
where
    F: Future<Output = ()>,
{
    let (result, ()) = tokio::join!(runtime.shutdown(), driver.as_mut());
    result
}

#[derive(Debug, Error)]
pub enum BurokkuError {
    #[error(transparent)]
    Window(#[from] winit::Error),

    #[error(transparent)]
    JavaScript(#[from] runtime::Error),

    #[error("the application script did not commit a Window under app")]
    MissingWindow,

    #[error("the main JavaScript runtime stopped before the native event loop")]
    MainRuntimeStopped,

    #[error("application host failed: {0}")]
    Host(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        runtime::{Runtime, RuntimeRole},
        ui::{
            layout::{LayoutEngine, LogicalViewport},
            scene::{PaintItem, ScenePlan},
        },
    };

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
        let committed = Arc::new(Notify::new());
        let notifier = Arc::clone(&committed);
        let (dom, publications) = DomPlugin::new(move |_| notifier.notify_one());
        let initial_revision = publications.load().revision();
        let runtime = Runtime::builder()
            .role(RuntimeRole::Background)
            .plugin(JsonPlugin)
            .plugin(dom)
            .build()
            .await
            .unwrap();

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
        let publication = wait_for_initial_window(&publications, initial_revision, &committed)
            .await
            .unwrap();
        let spec = WindowSpec::from_publication(&publication).unwrap().unwrap();

        assert_eq!(spec.title(), "Script host");
        assert_eq!(
            publication
                .snapshot()
                .children(spec.dom_id())
                .unwrap()
                .len(),
            1
        );

        let mut text = TextEngine::without_system_fonts();
        text.register_font_data(include_bytes!("../testdata/fonts/NotoSans-Regular.ttf").to_vec())
            .unwrap();
        let mut layout = LayoutEngine::new(text);
        let computed = layout
            .compute(
                Arc::clone(&publication),
                LogicalViewport::new(800.0, 600.0).unwrap(),
            )
            .unwrap();
        let plan =
            ScenePlan::from_layout(computed, winit::PhysicalSize::new(800, 600), 1.0).unwrap();
        assert_eq!(plan.revision(), publication.revision());
        assert!(plan
            .items()
            .iter()
            .any(|item| matches!(item, PaintItem::Text { .. })));

        runtime.shutdown().await.unwrap();
    }
}
