//! A thread-affine asynchronous JavaScript runtime backed by rquickjs and Tokio.

pub mod deserializer;
mod event_loop;
mod js_options;
mod plugin;
pub mod plugins;
mod runtime;
pub mod serializer;

pub use event_loop::{MacrotaskQueue, MacrotaskQueueError, DEFAULT_MACROTASK_CAPACITY};
pub use js_options::JsOptions;
pub use plugin::{Plugin, RuntimeBuilder};
pub use rquickjs;
pub use rquickjs::Error;
pub use runtime::{Runtime, RuntimeDriver};

/// The result type returned by this crate.
pub type Result<T> = rquickjs::Result<T>;
