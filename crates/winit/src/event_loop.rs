use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Notify;

use crate::{Window, WindowAttributes, WindowEvent, WindowId};

const POLL_INTERVAL: Duration = Duration::from_millis(4);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ControlFlow {
    Poll,
    #[default]
    Wait,
    WaitUntil(Instant),
}

/// Receives native application and window events.
pub trait ApplicationHandler {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {}
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EventLoopWaker {
    notified: Arc<Notify>,
}

impl EventLoopWaker {
    pub(crate) fn wake_up(&self) {
        self.notified.notify_one();
    }
}

/// A thread-safe handle for promptly waking an idle native event loop.
///
/// Waking does not itself dispatch an application event. It causes the loop to
/// pump native events and invoke `about_to_wait`, where cross-thread state can
/// be consumed without busy polling.
#[derive(Clone, Debug)]
pub struct EventLoopProxy {
    waker: EventLoopWaker,
}

impl EventLoopProxy {
    pub fn wake_up(&self) {
        self.waker.wake_up();
    }
}

struct ActiveEventLoopState {
    control_flow: Cell<ControlFlow>,
    exiting: Cell<bool>,
}

#[derive(Clone)]
pub struct ActiveEventLoop {
    state: Rc<ActiveEventLoopState>,
    platform: Weak<RefCell<crate::platform::PlatformEventLoop>>,
    waker: EventLoopWaker,
}

impl ActiveEventLoop {
    /// Create and show a native window on the event-loop thread.
    ///
    /// Call this from `resumed` or `about_to_wait`. Creating a window directly
    /// from a native `window_event` callback is not supported because the
    /// platform event pump is already borrowed for dispatch.
    pub fn create_window(&self, attributes: WindowAttributes) -> crate::Result<Window> {
        self.platform
            .upgrade()
            .ok_or(crate::Error::EventLoopUnavailable)?
            .borrow_mut()
            .create_window(attributes, self.waker.clone())
    }

    pub fn set_control_flow(&self, control_flow: ControlFlow) {
        self.state.control_flow.set(control_flow);
    }

    pub fn control_flow(&self) -> ControlFlow {
        self.state.control_flow.get()
    }

    pub fn exit(&self) {
        self.state.exiting.set(true);
    }

    pub fn exiting(&self) -> bool {
        self.state.exiting.get()
    }
}

pub struct EventLoop {
    active: ActiveEventLoop,
    waker: EventLoopWaker,
    has_run: bool,
    platform: Rc<RefCell<crate::platform::PlatformEventLoop>>,
}

impl EventLoop {
    pub fn new() -> crate::Result<Self> {
        let platform = Rc::new(RefCell::new(crate::platform::PlatformEventLoop::new()?));
        let waker = EventLoopWaker::default();

        Ok(Self {
            active: ActiveEventLoop {
                state: Rc::new(ActiveEventLoopState {
                    control_flow: Cell::new(ControlFlow::Wait),
                    exiting: Cell::new(false),
                }),
                platform: Rc::downgrade(&platform),
                waker: waker.clone(),
            },
            waker,
            has_run: false,
            platform,
        })
    }

    /// Create and show a native window.
    ///
    /// This must be called on the platform event-loop thread, before or during
    /// [`run_app`](Self::run_app). On macOS, that is the process main thread.
    pub fn create_window(&mut self, attributes: WindowAttributes) -> crate::Result<Window> {
        self.active.create_window(attributes)
    }

    /// Flush pending native window ordering before the asynchronous event loop
    /// starts. This is useful when renderer initialization happens after the
    /// first Window is created.
    pub fn flush_windows(&self) {
        self.platform.borrow().flush_windows();
    }

    /// Return a thread-safe handle that wakes this event loop from `Wait`.
    pub fn create_proxy(&self) -> EventLoopProxy {
        EventLoopProxy {
            waker: self.waker.clone(),
        }
    }

    /// Drive the native event queue as part of the current Tokio runtime.
    ///
    /// This future is intentionally `!Send` because native event loops are
    /// thread-affine (AppKit requires the process main thread). Drive it
    /// directly from the runtime's main `block_on` future; other `Send` tasks
    /// may use Tokio worker threads normally.
    /// The handler is returned after the loop exits so callers can inspect
    /// application state or deferred errors.
    pub async fn run_app<A: ApplicationHandler + 'static>(
        &mut self,
        application: A,
    ) -> crate::Result<A> {
        if self.has_run {
            return Err(crate::Error::AlreadyRun);
        }
        self.has_run = true;

        let application = Rc::new(RefCell::new(application));

        self.platform.borrow().set_handler({
            let application = application.clone();
            let active = self.active.clone();
            move |window_id, event| {
                application
                    .borrow_mut()
                    .window_event(&active, window_id, event);
            }
        });

        application.borrow_mut().resumed(&self.active);

        while !self.active.exiting() {
            self.platform.borrow_mut().pump();

            application.borrow_mut().about_to_wait(&self.active);
            if self.active.exiting() {
                break;
            }

            match self.active.control_flow() {
                ControlFlow::Poll => tokio::task::yield_now().await,
                ControlFlow::Wait => {
                    tokio::select! {
                        _ = self.waker.notified.notified() => {}
                        _ = tokio::time::sleep(POLL_INTERVAL) => {}
                    }
                }
                ControlFlow::WaitUntil(deadline) => {
                    let now = Instant::now();
                    if deadline <= now {
                        tokio::task::yield_now().await;
                    } else {
                        tokio::select! {
                            _ = self.waker.notified.notified() => {}
                            _ = tokio::time::sleep(
                                deadline.saturating_duration_since(now).min(POLL_INTERVAL),
                            ) => {}
                        }
                    }
                }
            }
        }

        application.borrow_mut().exiting(&self.active);

        self.platform.borrow().clear_handler();

        match Rc::try_unwrap(application) {
            Ok(application) => Ok(application.into_inner()),
            Err(_) => unreachable!("the platform event handler retains the application"),
        }
    }
}
