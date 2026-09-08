//! Asynchronous GPU initialization, replacement, cancellation, and installation.

use std::rc::Rc;

use tokio::sync::oneshot;
use winit::{ActiveEventLoop, WindowId};

use super::{
    super::{
        elements::NodeId,
        gpu::{GraphicsContext, GraphicsError, WindowRenderer},
        window_host::{PreparedWindow, WindowSpec},
    },
    ApplicationHost, HostError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PendingWindowStatus {
    Current,
    Removed,
    Replaced,
}

pub(super) fn pending_window_status(
    pending_dom_id: NodeId,
    desired_dom_id: Option<NodeId>,
) -> PendingWindowStatus {
    match desired_dom_id {
        Some(desired_dom_id) if desired_dom_id == pending_dom_id => PendingWindowStatus::Current,
        Some(_) => PendingWindowStatus::Replaced,
        None => PendingWindowStatus::Removed,
    }
}

pub(super) fn accept_current_graphics_result<T, E>(
    status: PendingWindowStatus,
    result: Result<T, E>,
) -> Option<Result<T, E>> {
    (status == PendingWindowStatus::Current).then_some(result)
}

pub(super) fn graphics_initialization_stop_is_fatal(status: PendingWindowStatus) -> bool {
    status == PendingWindowStatus::Current
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ReplacementRenderer<R> {
    Reuse(R),
    SelectCompatibleAdapter,
}

pub(super) fn classify_replacement_renderer<R>(
    result: Result<R, GraphicsError>,
) -> Result<ReplacementRenderer<R>, GraphicsError> {
    match result {
        Ok(renderer) => Ok(ReplacementRenderer::Reuse(renderer)),
        Err(GraphicsError::UnsupportedSurface) => Ok(ReplacementRenderer::SelectCompatibleAdapter),
        Err(error) => Err(error),
    }
}

type GraphicsInitialization = Result<(GraphicsContext, WindowRenderer), GraphicsError>;

#[derive(Debug)]
pub(super) struct AbortOnDrop(Option<tokio::task::JoinHandle<()>>);

impl AbortOnDrop {
    pub(super) fn new(task: tokio::task::JoinHandle<()>) -> Self {
        Self(Some(task))
    }

    fn take(&mut self) -> tokio::task::JoinHandle<()> {
        self.0.take().expect("abort-on-drop task remains installed")
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(task) = self.0.as_ref() {
            task.abort();
        }
    }
}

pub(super) async fn cancel_graphics_task<T, W>(
    task: tokio::task::JoinHandle<()>,
    result: oneshot::Receiver<T>,
    window: W,
) {
    task.abort();
    let _ = task.await;
    drop(result);
    drop(window);
}

#[derive(Debug)]
pub(super) struct PendingGraphicsInitialization {
    revision: u64,
    dom_id: NodeId,
    window_id: WindowId,
    result: oneshot::Receiver<GraphicsInitialization>,
    _task: AbortOnDrop,
}

#[derive(Debug)]
pub(super) struct PendingGraphicsReplacement {
    pub(super) revision: u64,
    // Drop any queued renderer before closing its candidate Window.
    result: oneshot::Receiver<GraphicsInitialization>,
    _task: AbortOnDrop,
    pub(super) prepared: PreparedWindow,
}

impl ApplicationHost {
    pub(super) fn begin_graphics_initialization(
        &mut self,
        event_loop: &ActiveEventLoop,
        revision: u64,
        dom_id: NodeId,
        window: Rc<winit::Window>,
    ) {
        debug_assert!(self.graphics.is_none());
        debug_assert!(self.renderer.is_none());
        debug_assert!(self.pending_graphics.is_none());

        let window_id = window.id();
        let waker = event_loop.loop_waker();
        let (sender, result) = oneshot::channel();
        let task = tokio::task::spawn_local(async move {
            let initialized = GraphicsContext::for_window(window).await;
            let _ = sender.send(initialized);
            waker.wake_up();
        });
        self.pending_graphics = Some(PendingGraphicsInitialization {
            revision,
            dom_id,
            window_id,
            result,
            _task: AbortOnDrop::new(task),
        });
    }

    pub(super) fn begin_graphics_replacement(
        &mut self,
        event_loop: &ActiveEventLoop,
        revision: u64,
        prepared: PreparedWindow,
    ) {
        debug_assert!(self.graphics.is_some());
        debug_assert!(self.renderer.is_some());
        debug_assert!(self.pending_graphics.is_none());
        debug_assert!(self.pending_graphics_replacement.is_none());

        let window = Rc::clone(prepared.window());
        let waker = event_loop.loop_waker();
        let (sender, result) = oneshot::channel();
        let task = tokio::task::spawn_local(async move {
            let initialized = GraphicsContext::for_window(window).await;
            let _ = sender.send(initialized);
            waker.wake_up();
        });
        self.pending_graphics_replacement = Some(PendingGraphicsReplacement {
            revision,
            prepared,
            result,
            _task: AbortOnDrop::new(task),
        });
    }

    pub(super) fn complete_graphics_initialization(&mut self) -> Result<(), HostError> {
        let Some(pending) = self.pending_graphics.as_mut() else {
            return Ok(());
        };

        let received = pending.result.try_recv();

        // Validate the asynchronous result against the current live DOM before
        // installing it. The borrow ends before any renderer or native work.
        let desired_dom_id = {
            let state = self
                .dom_bindings
                .try_borrow()
                .map_err(|_| HostError::DomBorrowConflict)?;
            WindowSpec::from_dom(&state.dom)?
                .as_ref()
                .map(WindowSpec::dom_id)
        };
        let status = pending_window_status(pending.dom_id, desired_dom_id);
        let initialized = match received {
            Ok(initialized) => initialized,
            Err(oneshot::error::TryRecvError::Empty) => return Ok(()),
            Err(oneshot::error::TryRecvError::Closed)
                if !graphics_initialization_stop_is_fatal(status) =>
            {
                return Ok(());
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                return Err(HostError::GraphicsInitializationStopped);
            }
        };
        let Some(initialized) = accept_current_graphics_result(status, initialized) else {
            return Ok(());
        };

        let pending = self
            .pending_graphics
            .take()
            .expect("completed graphics initialization remains installed");

        if self.windows.current().is_none_or(|window| {
            window.dom_id() != pending.dom_id || window.id() != pending.window_id
        }) {
            return Ok(());
        }

        match initialized {
            Ok((graphics, renderer)) => {
                self.graphics = Some(graphics);
                self.renderer = Some(renderer);
                self.ever_had_window = true;
                if let Some(window) = self.windows.current() {
                    window.window().request_redraw();
                }
                Ok(())
            }
            Err(error) => self.handle_window_sync_failure(pending.revision, error.into()),
        }
    }

    pub(super) fn complete_graphics_replacement(&mut self) -> Result<(), HostError> {
        let Some(pending) = self.pending_graphics_replacement.as_mut() else {
            return Ok(());
        };

        let received = pending.result.try_recv();
        if matches!(received, Err(oneshot::error::TryRecvError::Empty)) {
            return Ok(());
        }

        let is_current = {
            let state = self
                .dom_bindings
                .try_borrow()
                .map_err(|_| HostError::DomBorrowConflict)?;
            WindowSpec::from_dom(&state.dom)?.as_ref() == Some(pending.prepared.spec())
        };
        let pending = self
            .pending_graphics_replacement
            .take()
            .expect("completed graphics replacement remains installed");
        if !is_current {
            drop(received);
            drop(pending);
            return Ok(());
        }

        let initialized = match received {
            Ok(initialized) => initialized,
            Err(oneshot::error::TryRecvError::Closed) => {
                return self.handle_window_sync_failure(
                    pending.revision,
                    HostError::GraphicsInitializationStopped,
                );
            }
            Err(oneshot::error::TryRecvError::Empty) => unreachable!(),
        };
        match initialized {
            Ok((graphics, renderer)) => {
                self.queue_pointer_cancel()?;
                self.graphics.replace(graphics);
                let (previous_window, previous_renderer) =
                    pending
                        .prepared
                        .commit_with(&mut self.windows, &mut self.renderer, renderer);
                self.discard_stale_presented_frame();
                drop(previous_renderer); // releases surface before window
                if let Some(previous_window) = previous_window {
                    previous_window.close();
                }
                self.ever_had_window = true;
                if let Some(window) = self.windows.current() {
                    window.window().request_redraw();
                }
                Ok(())
            }
            Err(error) => self.handle_window_sync_failure(pending.revision, error.into()),
        }
    }

    pub(super) fn cancel_graphics_initialization(&mut self) {
        drop(self.pending_graphics.take());
        self.cancel_graphics_replacement();
    }

    pub(super) fn cancel_graphics_replacement(&mut self) {
        let Some(pending) = self.pending_graphics_replacement.take() else {
            return;
        };
        let PendingGraphicsReplacement {
            result,
            _task: mut task_guard,
            prepared,
            ..
        } = pending;
        let task = task_guard.take();
        tokio::task::spawn_local(cancel_graphics_task(task, result, prepared));
    }
}
