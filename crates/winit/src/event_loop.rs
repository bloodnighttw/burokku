use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::{
    runtime::{Builder, Runtime, RuntimeFlavor},
    sync::Notify,
    task::LocalSet,
};

use crate::{platform::PlatformTick, Window, WindowAttributes, WindowEvent, WindowId};

const POLL_INTERVAL: Duration = Duration::from_millis(4);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ControlFlow {
    Poll,
    #[default]
    Wait,
    WaitUntil(Instant),
}

fn external_deadline(
    control_flow: ControlFlow,
    tokio_deadline: Option<Instant>,
) -> Option<Instant> {
    match control_flow {
        ControlFlow::Poll | ControlFlow::Wait => tokio_deadline,
        ControlFlow::WaitUntil(deadline) => {
            Some(tokio_deadline.map_or(deadline, |tokio_deadline| tokio_deadline.min(deadline)))
        }
    }
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

#[derive(Clone, Debug)]
pub(crate) struct EventLoopWaker {
    notified: Arc<Notify>,
    platform: crate::platform::PlatformWake,
}

impl EventLoopWaker {
    fn new(platform: crate::platform::PlatformWake) -> Self {
        Self {
            notified: Arc::new(Notify::new()),
            platform,
        }
    }

    pub(crate) fn wake_up(&self) {
        self.notified.notify_one();
        self.platform.wake_up();
    }
}

/// Buffers native events emitted while an application callback is already
/// active. Events emitted by the native platform pump are otherwise delivered
/// immediately so nested platform loops, including macOS live resize, can keep
/// the application surface and layout synchronized.
#[derive(Clone, Default)]
struct WindowEventQueue {
    pending: Rc<RefCell<VecDeque<(WindowId, WindowEvent)>>>,
}

impl WindowEventQueue {
    fn push(&self, window_id: WindowId, event: WindowEvent) {
        self.pending.borrow_mut().push_back((window_id, event));
    }

    #[cfg(test)]
    fn handler(&self) -> impl FnMut(WindowId, WindowEvent) + 'static {
        let queue = self.clone();
        move |window_id, event| queue.push(window_id, event)
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

fn dispatch_or_defer<A>(
    application: &RefCell<A>,
    deferred: &WindowEventQueue,
    window_id: WindowId,
    event: WindowEvent,
    dispatch: impl FnOnce(&mut A, WindowId, WindowEvent),
) {
    let Ok(mut application) = application.try_borrow_mut() else {
        deferred.push(window_id, event);
        return;
    };
    dispatch(&mut application, window_id, event);
}

/// Clears an installed native event handler on every exit path, including
/// cancellation while [`EventLoop::run_app`] is suspended at an await point.
struct EventHandlerGuard<F: FnOnce()> {
    clear: Option<F>,
}

impl<F: FnOnce()> EventHandlerGuard<F> {
    fn new(clear: F) -> Self {
        Self { clear: Some(clear) }
    }

    fn clear(&mut self) {
        if let Some(clear) = self.clear.take() {
            clear();
        }
    }
}

impl<F: FnOnce()> Drop for EventHandlerGuard<F> {
    fn drop(&mut self) {
        self.clear();
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

impl tokio::runtime::ExternalWake for EventLoopProxy {
    fn wake(&self) {
        self.wake_up();
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
            .borrow()
            .create_window(attributes, self.waker.clone())
    }

    /// Flush pending native window ordering before renderer initialization.
    pub fn flush_windows(&self) {
        if let Some(platform) = self.platform.upgrade() {
            platform.borrow().flush_windows();
        }
    }

    /// Return a thread-safe handle that wakes this event loop from `Wait`.
    pub fn create_proxy(&self) -> EventLoopProxy {
        EventLoopProxy {
            waker: self.waker.clone(),
        }
    }

    pub fn set_control_flow(&self, control_flow: ControlFlow) {
        self.state.control_flow.set(control_flow);
        self.waker.wake_up();
    }

    pub fn control_flow(&self) -> ControlFlow {
        self.state.control_flow.get()
    }

    pub fn exit(&self) {
        self.state.exiting.set(true);
        self.waker.wake_up();
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
        let waker = EventLoopWaker::new(platform.borrow().waker());

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
        self.active.create_proxy()
    }

    /// Create a patched Tokio current-thread builder wired to this event loop.
    ///
    /// Callers may enable drivers and tune the external tick budget before
    /// building, then pass the resulting runtime to
    /// [`run_app_external`](Self::run_app_external).
    pub fn external_runtime_builder(&self) -> Builder {
        let mut builder = Builder::new_current_thread();
        builder.external_event_loop(Arc::new(self.create_proxy()));
        builder
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
        let events = WindowEventQueue::default();
        self.platform.borrow().set_handler({
            let active = self.active.clone();
            let application = Rc::clone(&application);
            let deferred = events.clone();
            move |window_id, event| {
                dispatch_or_defer(
                    &application,
                    &deferred,
                    window_id,
                    event,
                    |application, window_id, event| {
                        application.window_event(&active, window_id, event);
                    },
                );
            }
        });
        let mut handler_guard = EventHandlerGuard::new({
            let platform = Rc::clone(&self.platform);
            move || platform.borrow().clear_handler()
        });

        application.borrow_mut().resumed(&self.active);
        events.drain(|window_id, event| {
            application
                .borrow_mut()
                .window_event(&self.active, window_id, event);
        });

        while !self.active.exiting() {
            // Platform callbacks dispatch immediately while the pump holds only
            // a shared platform borrow. This remains responsive inside nested
            // native loops such as macOS live resize.
            self.platform.borrow().pump();
            events.drain(|window_id, event| {
                application
                    .borrow_mut()
                    .window_event(&self.active, window_id, event);
            });

            application.borrow_mut().about_to_wait(&self.active);
            events.drain(|window_id, event| {
                application
                    .borrow_mut()
                    .window_event(&self.active, window_id, event);
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

        application.borrow_mut().exiting(&self.active);

        handler_guard.clear();

        match Rc::try_unwrap(application) {
            Ok(application) => Ok(application.into_inner()),
            Err(_) => unreachable!("the platform event handler retains the application"),
        }
    }

    /// Let the native main loop drive a patched Tokio current-thread runtime.
    ///
    /// This is the inverse of [`run_app`](Self::run_app): the platform owns the
    /// outer wait while native callbacks perform bounded nonblocking Tokio
    /// ticks. This keeps nested native loops responsive (for example, AppKit's
    /// live-resize loop) while local Tokio, QuickJS, or LLRT tasks continue to
    /// run on the event-loop thread.
    ///
    /// Prefer [`external_runtime_builder`](Self::external_runtime_builder),
    /// which creates a current-thread builder already connected to this event
    /// loop. `local_set` is retained and supplied to every tick until the native
    /// loop exits.
    ///
    /// The macOS backend is implemented today. Other platform backends can
    /// implement the same native wake, timer, and main-loop hooks without
    /// changing this API.
    pub fn run_app_external<A: ApplicationHandler + 'static>(
        &mut self,
        application: A,
        runtime: Runtime,
        local_set: LocalSet,
    ) -> crate::Result<A> {
        if self.has_run {
            return Err(crate::Error::AlreadyRun);
        }
        if runtime.handle().runtime_flavor() != RuntimeFlavor::CurrentThread {
            return Err(crate::Error::InvalidExternalRuntime);
        }
        self.has_run = true;

        let application = Rc::new(RefCell::new(application));
        let events = WindowEventQueue::default();
        self.platform.borrow().set_handler({
            let active = self.active.clone();
            let application = Rc::clone(&application);
            let deferred = events.clone();
            move |window_id, event| {
                dispatch_or_defer(
                    &application,
                    &deferred,
                    window_id,
                    event,
                    |application, window_id, event| {
                        application.window_event(&active, window_id, event);
                    },
                );
            }
        });
        let mut handler_guard = EventHandlerGuard::new({
            let platform = Rc::clone(&self.platform);
            move || platform.borrow().clear_handler()
        });

        application.borrow_mut().resumed(&self.active);
        events.drain(|window_id, event| {
            application
                .borrow_mut()
                .window_event(&self.active, window_id, event);
        });

        let runtime = RefCell::new(Some(runtime));
        let local_set = RefCell::new(Some(local_set));
        let run_result = self.platform.borrow().run_external(
            || {
                if self.active.exiting() {
                    return PlatformTick {
                        next_deadline: None,
                        exit: true,
                    };
                }

                let tick = {
                    let runtime = runtime.borrow();
                    let mut local_set = local_set.borrow_mut();
                    runtime
                        .as_ref()
                        .expect("external runtime remains available while ticking")
                        .tick_nonblocking_with_local_set(
                            local_set
                                .as_mut()
                                .expect("external LocalSet remains available while ticking"),
                        )
                };

                events.drain(|window_id, event| {
                    application
                        .borrow_mut()
                        .window_event(&self.active, window_id, event);
                });
                application.borrow_mut().about_to_wait(&self.active);
                events.drain(|window_id, event| {
                    application
                        .borrow_mut()
                        .window_event(&self.active, window_id, event);
                });

                let control_flow = self.active.control_flow();
                if control_flow == ControlFlow::Poll {
                    self.waker.wake_up();
                }
                let next_deadline = external_deadline(control_flow, tick.next_deadline);

                PlatformTick {
                    next_deadline,
                    exit: self.active.exiting(),
                }
            },
            || {
                // Cancel local/LLRT work first, then stop and join Tokio's
                // reactor while the native wake source is still retained.
                drop(local_set.borrow_mut().take());
                drop(runtime.borrow_mut().take());
            },
        );

        application.borrow_mut().exiting(&self.active);
        handler_guard.clear();
        run_result?;

        match Rc::try_unwrap(application) {
            Ok(application) => Ok(application.into_inner()),
            Err(_) => unreachable!("the platform event handler retains the application"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };

    use super::*;

    #[test]
    fn external_deadline_uses_the_earliest_application_or_tokio_timer() {
        let now = Instant::now();
        let application = now + Duration::from_secs(2);
        let tokio = now + Duration::from_secs(1);

        assert_eq!(
            external_deadline(ControlFlow::WaitUntil(application), Some(tokio)),
            Some(tokio)
        );
        assert_eq!(
            external_deadline(ControlFlow::WaitUntil(application), None),
            Some(application)
        );
        assert_eq!(
            external_deadline(ControlFlow::Wait, Some(tokio)),
            Some(tokio)
        );
    }

    #[test]
    fn event_loop_proxy_is_a_thread_safe_external_wake() {
        static_assertions::assert_impl_all!(
            EventLoopProxy: Send,
            Sync,
            tokio::runtime::ExternalWake
        );
    }

    #[test]
    fn cancelling_a_guarded_future_runs_handler_cleanup() {
        let cleared = Rc::new(Cell::new(false));
        let mut future = Box::pin({
            let cleared = Rc::clone(&cleared);
            async move {
                let _handler_guard = EventHandlerGuard::new(move || cleared.set(true));
                std::future::pending::<()>().await;
            }
        });
        let mut context = Context::from_waker(Waker::noop());

        assert_eq!(future.as_mut().poll(&mut context), Poll::Pending);
        assert!(!cleared.get());

        drop(future);
        assert!(cleared.get());
    }

    #[test]
    fn native_events_dispatch_immediately_when_the_application_is_idle() {
        let events = WindowEventQueue::default();
        let application = RefCell::new(Vec::new());

        dispatch_or_defer(
            &application,
            &events,
            WindowId(7),
            WindowEvent::Resized(crate::PhysicalSize::new(1024, 768)),
            |application, window_id, event| application.push((window_id, event)),
        );

        assert_eq!(
            *application.borrow(),
            [(
                WindowId(7),
                WindowEvent::Resized(crate::PhysicalSize::new(1024, 768))
            )]
        );
        assert!(events.pending.borrow().is_empty());
    }

    #[test]
    fn native_events_wait_until_the_application_callback_releases_its_borrow() {
        let events = WindowEventQueue::default();
        let application = RefCell::new(Vec::new());

        let active_callback = application.borrow_mut();
        dispatch_or_defer(
            &application,
            &events,
            WindowId(7),
            WindowEvent::Focused(true),
            |application, window_id, event| application.push((window_id, event)),
        );
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
