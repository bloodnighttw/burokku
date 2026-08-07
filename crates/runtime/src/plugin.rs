//! Plugin declarations and runtime construction.

use crate::{plugins, Result, Runtime};
use rquickjs::Ctx;

/// A host integration installed into each runtime context.
///
/// Plugins normally register globals, functions, or runtime userdata. Async
/// integrations can use [`crate::MacrotaskQueue`] to schedule JavaScript work.
/// A function is itself a plugin, so small integrations need no wrapper type:
///
/// ```
/// use runtime::{rquickjs::{prelude::Func, Ctx}, Result, Runtime};
///
/// fn answer_plugin(context: &Ctx<'_>) -> Result<()> {
///     context.globals().set("answer", Func::from(|| 42))
/// }
///
/// # async fn example() -> Result<()> {
/// let runtime = Runtime::builder()
///     .plugin(answer_plugin)
///     .build()
///     .await?;
/// assert_eq!(runtime.eval::<i32>("answer()").await?, 42);
/// # Ok(())
/// # }
/// ```
pub trait Plugin: Send + 'static {
    /// A diagnostic name for this plugin.
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Install the plugin in a newly created QuickJS context.
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()>;
}

impl<F> Plugin for F
where
    F: for<'js> Fn(&Ctx<'js>) -> Result<()> + Send + 'static,
{
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
        self(context)
    }
}

/// Configures and creates a [`Runtime`].
pub struct RuntimeBuilder {
    pub(crate) plugins: Vec<Box<dyn Plugin>>,
}

impl RuntimeBuilder {
    /// Create a builder containing all standard runtime plugins.
    pub fn new() -> Self {
        Self {
            plugins: vec![
                Box::new(plugins::ConsolePlugin),
                Box::new(plugins::TimersPlugin),
                Box::new(plugins::WindowEventsPlugin),
            ],
        }
    }

    /// Create a builder without any plugins.
    ///
    /// The macrotask queue and QuickJS microtask handling are runtime features,
    /// so they remain available in a bare runtime.
    pub fn bare() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Add a plugin to this runtime.
    pub fn plugin<P>(mut self, plugin: P) -> Self
    where
        P: Plugin,
    {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// Build the configured runtime.
    pub async fn build(self) -> Result<Runtime> {
        Runtime::build(self, |_| Ok(())).await
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
            .finish()
    }
}
