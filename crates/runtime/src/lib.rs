//! An asynchronous JavaScript runtime backed by rquickjs and Tokio.

mod event_loop;
mod host;
mod runtime;
mod task;

pub use rquickjs;
pub use rquickjs::Error;
pub use runtime::{InputState, ModifiersState, MouseButton, Runtime, WindowEventMessage};

/// The result type returned by this crate.
pub type Result<T> = rquickjs::Result<T>;
