//! Main-thread application host and its focused subsystems.

mod events;
mod gpu_lifecycle;
mod reconcile;
mod render;

pub(crate) use events::{
    ChangedMouseButton, NativeKeyboardEvent, NativeKeyboardEventKind, NativeMouseInput,
    NativeMouseInputKind, PressedMouseButtons, WheelDeltaMode,
};

use thiserror::Error;
use winit::{ActiveEventLoop, PhysicalPosition, WindowId};

use crate::app::RuntimeLifecycle;

use super::{
    gpu::{GraphicsContext, GraphicsError, WindowRenderer},
    js_bindings::SharedDomBindings,
    layout::{LayoutEngine, LayoutError},
    resize_observer::ResizeObserverRegistry,
    scene::SceneError,
    text::TextEngine,
    window_host::{WindowHostError, WindowManager},
};

use self::{
    gpu_lifecycle::{PendingGraphicsInitialization, PendingGraphicsReplacement},
    render::{FrameFailure, PresentedFrame},
};

pub(crate) struct ApplicationHost {
    dom_bindings: SharedDomBindings,
    // Shared handle to the DOM's registry, also usable during borrow-error cleanup.
    resize_observers: ResizeObserverRegistry,
    observed_revision: Option<u64>,
    graphics: Option<GraphicsContext>,
    pending_graphics: Option<PendingGraphicsInitialization>,
    pending_graphics_replacement: Option<PendingGraphicsReplacement>,
    renderer: Option<WindowRenderer>,
    windows: WindowManager,
    layout: LayoutEngine<TextEngine>,
    presented: Option<PresentedFrame>,
    last_frame_failure: Option<FrameFailure>,
    hover_window: Option<WindowId>,
    cursor: Option<(PhysicalPosition<f64>, u16)>,
    ever_had_window: bool,
    fatal_error: Option<HostError>,
    lifecycle: RuntimeLifecycle,
    exit_requested: bool,
}

impl ApplicationHost {
    pub(crate) fn new(
        dom_bindings: SharedDomBindings,
        text: TextEngine,
        lifecycle: RuntimeLifecycle,
    ) -> Self {
        let resize_observers = dom_bindings.borrow().dom.resize_observers.clone();
        Self {
            dom_bindings,
            resize_observers,
            observed_revision: None,
            // GPU allocation is delayed until a native Window exists, so its
            // surface can constrain adapter selection.
            graphics: None,
            pending_graphics: None,
            pending_graphics_replacement: None,
            renderer: None,
            windows: WindowManager::default(),
            layout: LayoutEngine::new(text),
            presented: None,
            last_frame_failure: None,
            hover_window: None,
            cursor: None,
            ever_had_window: false,
            fatal_error: None,
            lifecycle,
            exit_requested: false,
        }
    }

    pub(crate) fn fatal_error(&self) -> Option<&HostError> {
        self.fatal_error.as_ref()
    }

    fn record_frame_failure(&mut self, failure: FrameFailure) {
        eprintln!(
            "Burokku warning: {:?} for DOM revision {} failed; continuing: {}",
            failure.stage, failure.revision, failure.message
        );
        self.last_frame_failure = Some(failure);
    }

    fn request_exit(&mut self) {
        self.exit_requested = true;
        self.resize_observers.shutdown();
        self.lifecycle.request_shutdown();
    }

    fn fail(&mut self, _event_loop: &ActiveEventLoop, error: HostError) {
        if self.fatal_error.is_none() {
            self.fatal_error = Some(error);
        }
        self.cancel_graphics_initialization();
        self.renderer = None;
        self.windows.close();
        self.request_exit();
    }
}

impl Drop for ApplicationHost {
    fn drop(&mut self) {
        self.resize_observers.shutdown();
    }
}

#[derive(Debug, Error)]
pub(crate) enum HostError {
    #[error("the live DOM is already borrowed by reentrant work")]
    DomBorrowConflict,

    #[error("live DOM maintenance failed: {0}")]
    DomMaintenance(String),

    #[error("the live DOM Window has no native window")]
    MissingNativeWindow,

    #[error("GPU initialization stopped before producing a renderer")]
    GraphicsInitializationStopped,

    #[error("the native Window has no GPU context")]
    MissingGraphicsContext,

    #[error("the native Window has no GPU renderer")]
    MissingRenderer,

    #[error("the active native Window and renderer do not match")]
    WindowRendererMismatch,

    #[error("native display scale factor must be positive and finite, got {0}")]
    InvalidScaleFactor(f64),

    #[error("logical viewport {width}x{height} exceeds f32 coordinates")]
    ViewportTooLarge { width: f64, height: f64 },

    #[error(transparent)]
    Window(#[from] WindowHostError),

    #[error(transparent)]
    Graphics(#[from] GraphicsError),

    #[error(transparent)]
    Layout(#[from] LayoutError),

    #[error(transparent)]
    Scene(#[from] SceneError),
}

#[cfg(test)]
mod tests;
