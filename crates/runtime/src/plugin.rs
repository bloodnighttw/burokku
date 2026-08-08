//! Plugin declarations and runtime construction.

use crate::{Result, Runtime, RuntimeDriver};
use rquickjs::{Ctx, JsLifetime};

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

/// Identifies the responsibility of one JavaScript isolate.
///
/// A role is stored as QuickJS userdata before plugins are installed, allowing
/// a plugin to expose different capabilities in different isolates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RuntimeRole {
    /// A regular, independently driven JavaScript runtime.
    #[default]
    Standalone,
    /// A latency-sensitive runtime colocated with the UI and rendering loop.
    Main,
    /// An off-main-thread runtime for application and business logic.
    Background,
}

unsafe impl<'js> JsLifetime<'js> for RuntimeRole {
    type Changed<'to> = RuntimeRole;
}

impl RuntimeRole {
    /// Read the role of the current isolate during plugin installation or use.
    pub fn from_context(context: &Ctx<'_>) -> Option<Self> {
        context.userdata::<Self>().map(|role| *role)
    }
}

/// Configures and creates a [`Runtime`].
pub struct RuntimeBuilder {
    pub(crate) plugins: Vec<Box<dyn Plugin>>,
    pub(crate) role: RuntimeRole,
}

impl RuntimeBuilder {
    /// Create a builder without plugins.
    ///
    /// Macrotask scheduling and QuickJS microtasks are runtime features. Host
    /// APIs such as console, timers, and window events must be added explicitly.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            role: RuntimeRole::Standalone,
        }
    }

    /// Assign a role to this isolate.
    pub fn role(mut self, role: RuntimeRole) -> Self {
        self.role = role;
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

    /// Build the configured runtime.
    pub async fn build(self) -> Result<Runtime> {
        Runtime::build(self, |_| Ok(())).await
    }

    /// Build without spawning the QuickJS driver.
    ///
    /// The returned driver must be continuously polled on the thread assigned
    /// to this isolate before evaluation or host tasks can make progress.
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
            .field("role", &self.role)
            .finish()
    }
}
