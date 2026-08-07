//! An asynchronous JavaScript runtime backed by rquickjs and Tokio.

mod event_loop;
mod plugin;
pub mod plugins;
mod runtime;

pub use event_loop::MacrotaskQueue;
pub use plugin::{Plugin, RuntimeBuilder};
pub use plugins::{InputState, ModifiersState, MouseButton, WindowEventMessage};
pub use rquickjs;
pub use rquickjs::Error;
pub use runtime::Runtime;

/// The result type returned by this crate.
pub type Result<T> = rquickjs::Result<T>;
