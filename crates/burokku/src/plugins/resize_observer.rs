//! JavaScript resize observation backed by the application's existing native DOM.

use runtime::{rquickjs::Ctx, Plugin};

use crate::ui::js_bindings;

/// Installs the JavaScript `ResizeObserver` constructor after [`super::dom::DomPlugin`].
/// `Burokku::run` installs this automatically. Standalone runtimes can opt in:
///
/// ```
/// use burokku::{
///     plugins::{dom::DomPlugin, resize_observer::ResizeObserverPlugin},
///     RuntimeBuilder,
/// };
///
/// let builder = RuntimeBuilder::new()
///     .plugin(DomPlugin::new())
///     .plugin(ResizeObserverPlugin);
/// ```
///
/// Installing the plugin does not create a native window or compute layout. A
/// native host must supply completed layouts for the same DOM. Entries report
/// layout width/height in logical pixels; delivery is not guaranteed before paint.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResizeObserverPlugin;

impl Plugin for ResizeObserverPlugin {
    fn name(&self) -> &'static str {
        "burokku-resize-observer"
    }

    fn install<'js>(&self, context: &Ctx<'js>) -> runtime::Result<()> {
        js_bindings::resize_observer::install(context)
    }
}
