//! Public application builder and end-to-end host assembly.

use thiserror::Error;

use crate::{
    runtime::{
        plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
        Plugin, Runtime, RuntimeBuilder,
    },
    ui::{dom_plugin::DomPlugin, host::ApplicationHost, text::TextEngine},
};

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

    /// Run JavaScript on BTS and drive the native application lifecycle.
    ///
    /// A Window does not need to be mounted at startup. The application stays
    /// active in a windowless state and reconciles a native window if a later
    /// DOM publication mounts one.
    ///
    /// This future must be polled on the process main thread. On macOS, use a
    /// current-thread Tokio runtime so AppKit, the main QuickJS isolate, layout,
    /// and presentation remain thread-affine.
    pub async fn run(self) -> Result<(), BurokkuError> {
        let local = tokio::task::LocalSet::new();
        local.run_until(self.run_local()).await
    }

    async fn run_local(self) -> Result<(), BurokkuError> {
        let mut event_loop = winit::EventLoop::new()?;
        let proxy = event_loop.create_proxy();
        let (dom_plugin, publications) = DomPlugin::new(move |_| proxy.wake_up());
        let (runtime, driver) = self.runtime.plugin(dom_plugin).build_driven().await?;
        let mut driver = tokio::task::spawn_local(driver.run());

        if let Err(error) = runtime.eval::<()>(self.script).await {
            let _ = shutdown_with_driver(runtime, &mut driver).await;
            return Err(BurokkuError::JavaScript(error));
        }

        let mut text = TextEngine::new();
        for font in self.fonts {
            if let Err(error) = text.register_font_data(font) {
                let _ = shutdown_with_driver(runtime, &mut driver).await;
                return Err(BurokkuError::Host(error.to_string()));
            }
        }

        let host = ApplicationHost::new(publications, text);
        let event_future = event_loop.run_app(host);
        tokio::pin!(event_future);
        let event_result = tokio::select! {
            result = &mut event_future => result,
            result = &mut driver => {
                result.map_err(|_| BurokkuError::MainRuntimeStopped)?;
                return Err(BurokkuError::MainRuntimeStopped);
            }
        };

        let shutdown_result = shutdown_with_driver(runtime, &mut driver).await;
        let host = event_result?;
        if let Some(error) = host.fatal_error() {
            let message = error.to_string();
            let _ = shutdown_result;
            return Err(BurokkuError::Host(message));
        }
        shutdown_result?;
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
        let runtime = RuntimeBuilder::new()
            .plugin(ConsolePlugin)
            .plugin(JsonPlugin)
            .plugin(TimersPlugin);
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

    /// Install an application plugin in the JavaScript runtime.
    pub fn runtime_plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.runtime = self.runtime.plugin(plugin);
        self
    }

    /// Install a plugin in the JavaScript runtime.
    pub fn main_runtime_plugin<P: Plugin>(mut self, plugin: P) -> Self {
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

async fn shutdown_with_driver(
    runtime: Runtime,
    driver: &mut tokio::task::JoinHandle<()>,
) -> runtime::Result<()> {
    let result = runtime.shutdown().await;
    let _ = driver.await;
    result
}

#[derive(Debug, Error)]
pub enum BurokkuError {
    #[error(transparent)]
    Window(#[from] winit::Error),

    #[error(transparent)]
    JavaScript(#[from] runtime::Error),

    #[error("the main JavaScript runtime stopped before the native event loop")]
    MainRuntimeStopped,

    #[error("application host failed: {0}")]
    Host(String),
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::sync::Notify;

    use super::*;
    use crate::{
        runtime::{plugins::TimersPlugin, Runtime},
        ui::{
            elements::{PublishedDom, PublishedDomReader},
            layout::{LayoutEngine, LogicalViewport},
            scene::{PaintItem, ScenePlan},
            window_host::WindowSpec,
        },
    };

    async fn next_publication(
        publications: &PublishedDomReader,
        after_revision: u64,
        committed: &Notify,
    ) -> Arc<PublishedDom> {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let publication = publications.load();
                if publication.revision() > after_revision {
                    return publication;
                }
                committed.notified().await;
            }
        })
        .await
        .expect("DOM publication did not arrive")
    }

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
        let publication = next_publication(&publications, initial_revision, &committed).await;
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

    #[tokio::test(flavor = "current_thread")]
    async fn detached_window_may_be_mounted_by_a_later_timer_publication() {
        let committed = Arc::new(Notify::new());
        let notifier = Arc::clone(&committed);
        let (dom, publications) = DomPlugin::new(move |_| notifier.notify_one());
        let initial_revision = publications.load().revision();
        let runtime = Runtime::builder()
            .plugin(TimersPlugin)
            .plugin(dom)
            .build()
            .await
            .unwrap();

        runtime
            .eval::<()>(
                "globalThis.pendingWindow = app.createElement('window');\n\
                 setTimeout(() => app.appendChild(pendingWindow), 100);",
            )
            .await
            .unwrap();

        let windowless = next_publication(&publications, initial_revision, &committed).await;
        assert_eq!(WindowSpec::from_publication(&windowless).unwrap(), None);

        let mounted = next_publication(&publications, windowless.revision(), &committed).await;
        assert!(WindowSpec::from_publication(&mounted).unwrap().is_some());

        runtime.shutdown().await.unwrap();
    }
}
