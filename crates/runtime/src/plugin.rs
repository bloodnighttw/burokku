//! Plugin declarations and runtime construction.

use crate::{Result, Runtime, RuntimeDriver, DEFAULT_MACROTASK_CAPACITY};
use rquickjs::Ctx;

/// A host integration installed into a runtime context.
///
/// Plugins register globals, functions, or runtime userdata. Async integrations
/// can use [`crate::JsTaskQueue`] to schedule JavaScript work. Plugins are
/// thread-local and are installed on the thread that drives QuickJS.
///
/// A function is itself a plugin, so small integrations need no wrapper type.
pub trait Plugin: 'static {
    /// A diagnostic name for this plugin.
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Install the plugin in a newly created QuickJS context.
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()>;
}

impl<F> Plugin for F
where
    F: for<'js> Fn(&Ctx<'js>) -> Result<()> + 'static,
{
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
        self(context)
    }
}

/// Configures and creates a thread-local [`Runtime`].
pub struct RuntimeBuilder {
    pub(crate) plugins: Vec<Box<dyn Plugin>>,
    pub(crate) macrotask_capacity: usize,
}

impl RuntimeBuilder {
    /// Create a builder without plugins.
    ///
    /// Macrotask scheduling and QuickJS microtasks are runtime features. Host
    /// APIs such as console and timers must be added explicitly.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            macrotask_capacity: DEFAULT_MACROTASK_CAPACITY,
        }
    }

    /// Set the maximum number of macrotasks waiting to run.
    pub fn macrotask_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "macrotask capacity must be non-zero");
        self.macrotask_capacity = capacity;
        self
    }

    /// Add a plugin to this runtime.
    pub fn plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin,
    {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// Build without spawning the thread-affine QuickJS driver.
    ///
    /// The returned driver must be continuously polled with
    /// [`tokio::task::spawn_local`] on one persistent [`tokio::task::LocalSet`].
    pub async fn build_driven(self) -> Result<(Runtime, RuntimeDriver)> {
        Runtime::build_driven(self, |_| Ok(())).await
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RuntimeBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeBuilder")
            .field(
                "plugins",
                &self
                    .plugins
                    .iter()
                    .map(|plugin| plugin.name())
                    .collect::<Vec<_>>(),
            )
            .field("macrotask_capacity", &self.macrotask_capacity)
            .finish()
    }
}
