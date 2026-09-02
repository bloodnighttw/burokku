use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::{pending, Future},
    rc::{Rc, Weak},
    sync::Arc,
    task::{Context, Wake, Waker},
    time::Instant,
};

use tokio::{
    runtime::{Builder, Runtime},
    task::LocalSet,
};

use crate::{platform::PlatformTick, Window, WindowAttributes, WindowEvent, WindowId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ControlFlow {
    Poll,
    #[default]
    Wait,
    WaitUntil(Instant),
}

fn external_deadline(control_flow: ControlFlow) -> Option<Instant> {
    match control_flow {
        ControlFlow::WaitUntil(deadline) => Some(deadline),
        ControlFlow::Poll | ControlFlow::Wait => None,
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
    platform: crate::platform::PlatformWake,
}

impl EventLoopWaker {
    fn new(platform: crate::platform::PlatformWake) -> Self {
        Self { platform }
    }

    pub(crate) fn wake_up(&self) {
        self.platform.wake_up();
    }
}

/// Buffers native events emitted while an application callback is already
/// active. Events emitted by native callbacks are otherwise delivered immediately
/// so nested platform loops, including macOS live resize, can keep
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
fn with_external_context<T>(
    runtime: &RefCell<Option<Runtime>>,
    local_set: &RefCell<Option<LocalSet>>,
    callback: impl FnOnce() -> T,
) -> T {
    let runtime = runtime.borrow();
    let _runtime_guard = runtime
        .as_ref()
        .expect("external runtime remains available during application callbacks")
        .enter();
    if let Ok(local_set) = local_set.try_borrow() {
        let _local_guard = local_set
            .as_ref()
            .expect("external LocalSet remains available during application callbacks")
            .enter();
        callback()
    } else {
        // A native event may be emitted reentrantly while a Tokio tick is
        // polling the LocalSet. That tick has already entered this exact local
        // context, so borrowing the LocalSet again is unnecessary.
        callback()
    }
}

/// Clears an installed native event handler on every exit path, including
/// application or runtime panics during [`EventLoop::run_app_external`].
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
/// Waking does not itself dispatch an application event. It schedules a bounded
/// Tokio tick and invokes `about_to_wait`, where cross-thread state can be consumed
/// without busy polling.
#[derive(Clone, Debug)]
pub struct EventLoopProxy {
    waker: EventLoopWaker,
}

impl EventLoopProxy {
    pub fn wake_up(&self) {
        self.waker.wake_up();
    }
}

impl Wake for EventLoopProxy {
    fn wake(self: Arc<Self>) {
        self.wake_up();
    }

    fn wake_by_ref(self: &Arc<Self>) {
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

    /// Return a thread-safe handle that wakes this event loop from `Wait`.
    pub fn create_proxy(&self) -> EventLoopProxy {
        self.active.create_proxy()
    }

    /// Let the native main loop drive an internally owned Tokio runtime.
    ///
    /// The platform owns the outer wait while native callbacks poll the persistent
    /// main-thread [`LocalSet`]. A one-worker Tokio runtime drives timers, I/O, and
    /// `Send` tasks, waking the native loop when local work becomes runnable.
    ///
    /// The macOS backend is implemented today. Other platform backends can
    /// implement the same native wake, timer, and main-loop hooks without
    /// changing this API.
    pub fn run_app_external<A: ApplicationHandler + 'static>(
        &mut self,
        application: A,
        local_set: LocalSet,
    ) -> crate::Result<A> {
        if self.has_run {
            return Err(crate::Error::AlreadyRun);
        }

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(crate::Error::ExternalRuntime)?;
        let local_waker = Waker::from(Arc::new(self.create_proxy()));
        self.has_run = true;

        let application = Rc::new(RefCell::new(application));
        let events = WindowEventQueue::default();
        let runtime = Rc::new(RefCell::new(Some(runtime)));
        let local_set = Rc::new(RefCell::new(Some(local_set)));
        self.platform.borrow().set_handler({
            let active = self.active.clone();
            let application = Rc::clone(&application);
            let deferred = events.clone();
            let runtime = Rc::clone(&runtime);
            let local_set = Rc::clone(&local_set);
            move |window_id, event| {
                with_external_context(&runtime, &local_set, || {
                    dispatch_or_defer(
                        &application,
                        &deferred,
                        window_id,
                        event,
                        |application, window_id, event| {
                            application.window_event(&active, window_id, event);
                        },
                    );
                });
            }
        });
        let mut handler_guard = EventHandlerGuard::new({
            let platform = Rc::clone(&self.platform);
            move || platform.borrow().clear_handler()
        });

        with_external_context(&runtime, &local_set, || {
            application.borrow_mut().resumed(&self.active);
            events.drain(|window_id, event| {
                application
                    .borrow_mut()
                    .window_event(&self.active, window_id, event);
            });
        });
        let run_result = self.platform.borrow().run_external(
            || {
                if self.active.exiting() {
                    return PlatformTick {
                        next_deadline: None,
                        exit: true,
                    };
                }

                {
                    let runtime = runtime.borrow();
                    let mut local_set = local_set.borrow_mut();
                    let _runtime_guard = runtime
                        .as_ref()
                        .expect("external runtime remains available while ticking")
                        .enter();
                    let mut local_future = std::pin::pin!(local_set
                        .as_mut()
                        .expect("external LocalSet remains available while ticking")
                        .run_until(pending::<()>()));
                    let mut context = Context::from_waker(&local_waker);
                    assert!(local_future.as_mut().poll(&mut context).is_pending());
                }
                with_external_context(&runtime, &local_set, || {
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
                });

                let control_flow = self.active.control_flow();
                if control_flow == ControlFlow::Poll {
                    self.waker.wake_up();
                }
                let next_deadline = external_deadline(control_flow);

                PlatformTick {
                    next_deadline,
                    exit: self.active.exiting(),
                }
            },
            || {
                with_external_context(&runtime, &local_set, || {
                    application.borrow_mut().exiting(&self.active);
                });
                // Cancel local/LLRT work before joining the Tokio worker while the
                // native wake source is still retained.
                drop(local_set.borrow_mut().take());
                drop(runtime.borrow_mut().take());
            },
        );

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
    use std::time::Duration;

    #[test]
    fn external_context_allows_callback_spawn_local() {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let local_set = LocalSet::new();
        let runtime = RefCell::new(Some(runtime));
        let local_set = RefCell::new(Some(local_set));

        let task = with_external_context(&runtime, &local_set, || {
            tokio::task::spawn_local(async { 42 })
        });
        let runtime = runtime.borrow_mut().take().unwrap();
        let local_set = local_set.borrow_mut().take().unwrap();
        let value = runtime.block_on(local_set.run_until(task)).unwrap();

        assert_eq!(value, 42);
    }

    use super::*;

    #[test]
    fn external_deadline_uses_only_the_application_deadline() {
        let deadline = Instant::now() + Duration::from_secs(2);

        assert_eq!(
            external_deadline(ControlFlow::WaitUntil(deadline)),
            Some(deadline)
        );
        assert_eq!(external_deadline(ControlFlow::Wait), None);
        assert_eq!(external_deadline(ControlFlow::Poll), None);
    }

    #[test]
    fn event_loop_proxy_is_a_thread_safe_waker() {
        static_assertions::assert_impl_all!(EventLoopProxy: Send, Sync, Wake);
    }

    #[test]
    fn dropping_guard_runs_handler_cleanup() {
        let cleared = Rc::new(Cell::new(false));
        {
            let guard_cleared = Rc::clone(&cleared);
            let _handler_guard = EventHandlerGuard::new(move || guard_cleared.set(true));
            assert!(!cleared.get());
        }

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
