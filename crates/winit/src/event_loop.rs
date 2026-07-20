use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

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

struct ActiveEventLoopState {
    control_flow: Cell<ControlFlow>,
    exiting: Cell<bool>,
}

#[derive(Clone)]
pub struct ActiveEventLoop {
    state: Rc<ActiveEventLoopState>,
}

impl ActiveEventLoop {
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
    has_run: bool,
    #[cfg(target_os = "macos")]
    platform: crate::platform::PlatformEventLoop,
}

impl EventLoop {
    pub fn new() -> crate::Result<Self> {
        #[cfg(target_os = "macos")]
        let platform = crate::platform::PlatformEventLoop::new()?;

        #[cfg(not(target_os = "macos"))]
        return Err(crate::Error::UnsupportedPlatform);

        #[cfg(target_os = "macos")]
        Ok(Self {
            active: ActiveEventLoop {
                state: Rc::new(ActiveEventLoopState {
                    control_flow: Cell::new(ControlFlow::Wait),
                    exiting: Cell::new(false),
                }),
            },
            has_run: false,
            platform,
        })
    }

    /// Create and show a native window.
    ///
    /// This must be called on the macOS main thread, before or during
    /// [`run_app`](Self::run_app).
    pub fn create_window(&mut self, attributes: WindowAttributes) -> crate::Result<Window> {
        #[cfg(target_os = "macos")]
        return self.platform.create_window(attributes);

        #[cfg(not(target_os = "macos"))]
        Err(crate::Error::UnsupportedPlatform)
    }

    /// Drive AppKit as part of the current Tokio runtime.
    ///
    /// This future is intentionally `!Send`: AppKit must remain on the process
    /// main thread. Drive it directly from the runtime's main `block_on`
    /// future; other `Send` tasks may use Tokio worker threads normally.
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

        #[cfg(target_os = "macos")]
        self.platform.set_handler({
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
            #[cfg(target_os = "macos")]
            self.platform.pump();

            application.borrow_mut().about_to_wait(&self.active);
            if self.active.exiting() {
                break;
            }

            match self.active.control_flow() {
                ControlFlow::Poll => tokio::task::yield_now().await,
                ControlFlow::Wait => tokio::time::sleep(POLL_INTERVAL).await,
                ControlFlow::WaitUntil(deadline) => {
                    let now = Instant::now();
                    if deadline <= now {
                        tokio::task::yield_now().await;
                    } else {
                        tokio::time::sleep(
                            deadline.saturating_duration_since(now).min(POLL_INTERVAL),
                        )
                        .await;
                    }
                }
            }
        }

        application.borrow_mut().exiting(&self.active);

        #[cfg(target_os = "macos")]
        self.platform.clear_handler();

        match Rc::try_unwrap(application) {
            Ok(application) => Ok(application.into_inner()),
            Err(_) => unreachable!("the platform event handler retains the application"),
        }
    }
}
