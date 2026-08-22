use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
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

/// Buffers native events until the event loop is outside both the platform
/// pump and the current application callback.
///
/// AppKit may synchronously emit events while an application callback creates
/// or resizes a window. The platform handler must therefore only enqueue: if it
/// called the application directly, it would reentrantly borrow the handler.
#[derive(Clone, Default)]
struct WindowEventQueue {
    pending: Rc<RefCell<VecDeque<(WindowId, WindowEvent)>>>,
}

impl WindowEventQueue {
    fn handler(&self) -> impl FnMut(WindowId, WindowEvent) + 'static {
        let queue = self.clone();
        move |window_id, event| queue.pending.borrow_mut().push_back((window_id, event))
    }

    fn drain(&self, mut dispatch: impl FnMut(WindowId, WindowEvent)) {
        loop {
            // Drop the queue borrow before dispatching. The application may
            // synchronously cause another native event while handling this one.
            let event = self.pending.borrow_mut().pop_front();
            let Some((window_id, event)) = event else {
                break;
            };
            dispatch(window_id, event);
        }
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
    /// Call this from an [`ApplicationHandler`] callback on the event-loop
    /// thread. Native events emitted synchronously during creation are queued
    /// until the current callback returns.
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

        let mut application = application;
        let events = WindowEventQueue::default();
        self.platform.borrow().set_handler(events.handler());

        application.resumed(&self.active);
        events.drain(|window_id, event| {
            application.window_event(&self.active, window_id, event);
        });

        while !self.active.exiting() {
            // Native callbacks only fill `events`, so application code runs
            // after the mutable platform borrow from `pump` has been released.
            self.platform.borrow_mut().pump();
            events.drain(|window_id, event| {
                application.window_event(&self.active, window_id, event);
            });

            application.about_to_wait(&self.active);
            events.drain(|window_id, event| {
                application.window_event(&self.active, window_id, event);
            });
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

        application.exiting(&self.active);

        self.platform.borrow().clear_handler();

        Ok(application)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_events_wait_until_the_application_callback_releases_its_borrow() {
        let events = WindowEventQueue::default();
        let mut native_handler = events.handler();
        let application = Rc::new(RefCell::new(Vec::new()));

        let active_callback = application.borrow_mut();
        native_handler(WindowId(7), WindowEvent::Focused(true));
        drop(active_callback);

        events.drain(|window_id, event| {
            application.borrow_mut().push((window_id, event));
        });

        assert_eq!(
            *application.borrow(),
            [(WindowId(7), WindowEvent::Focused(true))]
        );
    }

    #[test]
    fn events_emitted_during_dispatch_keep_fifo_order() {
        let events = WindowEventQueue::default();
        let mut native_handler = events.handler();
        native_handler(WindowId(1), WindowEvent::Focused(true));
        native_handler(WindowId(2), WindowEvent::Focused(false));

        let reentrant_events = events.clone();
        let mut received = Vec::new();
        events.drain(|window_id, event| {
            received.push((window_id, event));
            if window_id == WindowId(1) {
                reentrant_events
                    .pending
                    .borrow_mut()
                    .push_back((WindowId(3), WindowEvent::CloseRequested));
            }
        });

        assert_eq!(
            received,
            [
                (WindowId(1), WindowEvent::Focused(true)),
                (WindowId(2), WindowEvent::Focused(false)),
                (WindowId(3), WindowEvent::CloseRequested),
            ]
        );
    }
}
