//! Placeholder backend for targets that do not have a native implementation yet.

use std::fmt;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use crate::{
    event_loop::EventLoopWaker, Error, LogicalSize, Window, WindowAttributes, WindowEvent, WindowId,
};

use super::PlatformTick;

#[derive(Clone, Default)]
pub(crate) struct PlatformWake;

impl PlatformWake {
    pub(crate) fn wake_up(&self) {}
}

impl fmt::Debug for PlatformWake {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PlatformWake")
    }
}

pub(crate) struct PlatformEventLoop;

impl PlatformEventLoop {
    pub(crate) fn new() -> crate::Result<Self> {
        Err(Error::UnsupportedPlatform)
    }

    pub(crate) fn waker(&self) -> PlatformWake {
        PlatformWake
    }

    pub(crate) fn create_window(
        &self,
        _attributes: WindowAttributes,
        _event_loop_waker: EventLoopWaker,
    ) -> crate::Result<Window> {
        Err(Error::UnsupportedPlatform)
    }

    pub(crate) fn set_handler(&self, _handler: impl FnMut(WindowId, WindowEvent) + 'static) {}

    pub(crate) fn clear_handler(&self) {}

    pub(crate) fn flush_windows(&self) {}

    pub(crate) fn pump(&self) {}

    pub(crate) fn run_external(
        &self,
        _tick: impl FnMut() -> PlatformTick,
        shutdown: impl FnOnce(),
    ) -> crate::Result<()> {
        shutdown();
        Err(Error::UnsupportedPlatform)
    }
}

pub(crate) struct PlatformWindow;

impl PlatformWindow {
    pub(crate) fn request_redraw(&self) {}

    pub(crate) fn set_title(&self, _title: &str) {}

    pub(crate) fn set_inner_size(&self, _size: LogicalSize<f64>) {}

    pub(crate) fn close(&self) {}
}

impl HasWindowHandle for PlatformWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for PlatformWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}
