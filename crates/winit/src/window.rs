use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use crate::{event_loop::EventLoopWaker, LogicalSize, PhysicalSize};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WindowId(pub(crate) u64);

#[derive(Clone, Debug, PartialEq)]
pub struct WindowAttributes {
    pub(crate) title: String,
    pub(crate) inner_size: LogicalSize<f64>,
    pub(crate) resizable: bool,
}

impl Default for WindowAttributes {
    fn default() -> Self {
        Self {
            title: "Burokku".into(),
            inner_size: LogicalSize::new(800.0, 600.0),
            resizable: true,
        }
    }
}

impl WindowAttributes {
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_inner_size(mut self, size: LogicalSize<f64>) -> Self {
        self.inner_size = size;
        self
    }

    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }
}

#[derive(Debug)]
pub(crate) struct WindowState {
    pub(crate) id: WindowId,
    size: AtomicU64,
    scale_factor: AtomicU64,
    pub(crate) redraw_requested: AtomicBool,
    event_loop_waker: EventLoopWaker,
}

// The unsupported placeholder backend does not construct window state, while
// every native backend uses these shared state transitions.
impl WindowState {
    #[allow(dead_code)]
    pub(crate) fn new(
        id: WindowId,
        size: PhysicalSize<u32>,
        scale_factor: f64,
        event_loop_waker: EventLoopWaker,
    ) -> Self {
        Self {
            id,
            size: AtomicU64::new(pack_size(size)),
            scale_factor: AtomicU64::new(scale_factor.to_bits()),
            redraw_requested: AtomicBool::new(true),
            event_loop_waker,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_size(&self, size: PhysicalSize<u32>) {
        self.size.store(pack_size(size), Ordering::Release);
    }

    #[allow(dead_code)]
    pub(crate) fn set_scale_factor(&self, scale_factor: f64) {
        self.scale_factor
            .store(scale_factor.to_bits(), Ordering::Release);
    }

    pub(crate) fn size(&self) -> PhysicalSize<u32> {
        unpack_size(self.size.load(Ordering::Acquire))
    }

    pub(crate) fn scale_factor(&self) -> f64 {
        f64::from_bits(self.scale_factor.load(Ordering::Acquire))
    }
}

#[allow(dead_code)]
const fn pack_size(size: PhysicalSize<u32>) -> u64 {
    ((size.width as u64) << 32) | size.height as u64
}

const fn unpack_size(size: u64) -> PhysicalSize<u32> {
    PhysicalSize::new((size >> 32) as u32, size as u32)
}

/// A native window handle owned by the platform event-loop thread.
///
/// `Window` is deliberately neither [`Send`] nor [`Sync`]. Use
/// [`crate::EventLoopProxy`] to wake the event loop from other threads.
pub struct Window {
    pub(crate) state: std::sync::Arc<WindowState>,
    pub(crate) platform: crate::platform::PlatformWindow,
    // Native windows are owned by the platform event-loop thread. Keep this
    // handle thread-affine so operations and final release cannot synchronously
    // dispatch back to a main thread that may already be shutting down.
    pub(crate) _thread_affinity: PhantomData<Rc<()>>,
}

impl Window {
    pub fn default_attributes() -> WindowAttributes {
        WindowAttributes::default()
    }

    pub fn id(&self) -> WindowId {
        self.state.id
    }

    pub fn inner_size(&self) -> PhysicalSize<u32> {
        self.state.size()
    }

    pub fn scale_factor(&self) -> f64 {
        self.state.scale_factor()
    }

    pub fn request_redraw(&self) {
        self.state.redraw_requested.store(true, Ordering::Release);
        self.state.event_loop_waker.wake_up();
        self.platform.request_redraw();
    }

    /// Hook called immediately before presenting a frame.
    ///
    /// The current backend does not require additional work here, but keeping
    /// this hook makes renderers portable across winit and burokku-winit.
    pub fn pre_present_notify(&self) {}

    pub fn set_title(&self, title: &str) {
        self.platform.set_title(title);
    }

    /// Request a new logical content size.
    pub fn set_inner_size(&self, size: LogicalSize<f64>) -> crate::Result<()> {
        if !(size.width.is_finite()
            && size.height.is_finite()
            && size.width > 0.0
            && size.height > 0.0)
        {
            return Err(crate::Error::WindowCreation(
                "inner size must contain positive finite dimensions".into(),
            ));
        }
        self.platform.set_inner_size(size);
        Ok(())
    }

    /// Close the native window while retaining this handle until it is dropped.
    pub fn close(&self) {
        self.platform.close();
    }
}

impl std::fmt::Debug for Window {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Window")
            .field("id", &self.id())
            .field("inner_size", &self.inner_size())
            .field("scale_factor", &self.scale_factor())
            .finish_non_exhaustive()
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        self.platform.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        self.platform.display_handle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_builder_friendly() {
        let attributes = Window::default_attributes()
            .with_title("test")
            .with_inner_size(LogicalSize::new(640.0, 480.0))
            .with_resizable(false);

        assert_eq!(attributes.title, "test");
        assert_eq!(attributes.inner_size, LogicalSize::new(640.0, 480.0));
        assert!(!attributes.resizable);
    }

    #[test]
    fn physical_size_round_trips_through_atomic_storage() {
        let size = PhysicalSize::new(u32::MAX, 42);
        assert_eq!(unpack_size(pack_size(size)), size);
    }

    #[test]
    fn window_is_thread_affine() {
        static_assertions::assert_not_impl_any!(Window: Send, Sync);
    }
}
