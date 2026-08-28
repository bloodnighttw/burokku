//! A deliberately small windowing crate for Burokku.
//!
//! The API follows the useful parts of winit's application-handler model, but
//! [`EventLoop::run_app`] is asynchronous. Native events are drained in small
//! batches and the driver yields to Tokio whenever the native queue is idle.
//! [`EventLoop::run_app_external`] inverts ownership so a native main loop can
//! drive a patched Tokio current-thread runtime and `LocalSet` through shared
//! wake/timer hooks. macOS is currently implemented; other platform backends
//! can be added behind the crate's internal boundary without changing either
//! public API.

pub mod dpi;
pub mod event;
pub mod event_loop;
pub mod window;

mod platform;

pub mod application {
    pub use crate::event_loop::ApplicationHandler;
}

pub use dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
pub use event::{ElementState, KeyEvent, Modifiers, MouseButton, WindowEvent};
pub use event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
pub use raw_window_handle;
pub use window::{Window, WindowAttributes, WindowId};

/// Errors produced while creating or driving the native event loop.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the macOS event loop must be created and driven on the main thread")]
    NotMainThread,
    #[error("the event loop has already been run")]
    AlreadyRun,
    #[error("the external event loop requires a Tokio current-thread runtime")]
    InvalidExternalRuntime,
    #[error("the active event loop is no longer available")]
    EventLoopUnavailable,
    #[error("window creation failed: {0}")]
    WindowCreation(String),
    #[error("burokku-winit does not support this platform yet")]
    UnsupportedPlatform,
}

pub type Result<T> = std::result::Result<T, Error>;
