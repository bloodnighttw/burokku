//! An asynchronous JavaScript runtime backed by rquickjs and Tokio.

mod bridge;
pub mod deserializer;
mod dual_runtime;
mod event_loop;
mod js_options;
mod plugin;
pub mod plugins;
mod runtime;
pub mod serializer;

pub use bridge::{bridge_channel, BridgeEndpoint};
pub use dual_runtime::{DualRuntime, DualRuntimeBuilder, DualRuntimeDriver};
pub use event_loop::{MacrotaskQueue, MacrotaskQueueError, DEFAULT_MACROTASK_CAPACITY};
pub use js_options::JsOptions;
pub use plugin::{Plugin, RuntimeBuilder, RuntimeRole};
pub use rquickjs;
pub use rquickjs::Error;
pub use runtime::{Runtime, RuntimeDriver};

/// The result type returned by this crate.
pub type Result<T> = rquickjs::Result<T>;
