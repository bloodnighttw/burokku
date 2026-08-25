#[allow(
    dead_code,
    reason = "framework-fixture helpers are compiled only for optional integration bundles"
)]
pub(crate) mod dom_plugin;
pub mod elements;
pub(crate) mod gpu;
pub(crate) mod host;
pub(crate) mod layout;
pub(crate) mod scene;
pub(crate) mod text;
pub(crate) mod window_host;
