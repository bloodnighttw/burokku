//! A deliberately small windowing crate for Burokku.
//!
//! The API follows the useful parts of winit's application-handler model.
//! [`EventLoop::run_app_external`] keeps the native loop on the main thread while
//! upstream Tokio drives worker tasks and one persistent main-thread `LocalSet`.
//! macOS is currently implemented; other platform backends can be added behind the crate's
//! internal boundary without changing the public API.

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
    #[error("failed to build the external Tokio runtime: {0}")]
    ExternalRuntime(#[source] std::io::Error),
    #[error("the active event loop is no longer available")]
    EventLoopUnavailable,
    #[error("window creation failed: {0}")]
    WindowCreation(String),
    #[error("burokku-winit does not support this platform yet")]
    UnsupportedPlatform,
}

pub type Result<T> = std::result::Result<T, Error>;
