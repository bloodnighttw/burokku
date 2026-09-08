//! Reconcile live DOM window declarations with native windows and renderers.

use std::rc::Rc;

use winit::ActiveEventLoop;

use super::{
    super::{
        gpu::WindowRenderer,
        window_host::{WindowChange, WindowSpec},
    },
    gpu_lifecycle::{classify_replacement_renderer, ReplacementRenderer},
    render::{failure_policy, FailureKind, FailurePolicy, FrameFailure, FrameStage},
    ApplicationHost, HostError,
};

impl ApplicationHost {
    pub(super) fn handle_window_sync_failure(
        &mut self,
        revision: u64,
        error: HostError,
    ) -> Result<(), HostError> {
        let has_presented_frame = self.has_usable_presented_frame();
        if failure_policy(has_presented_frame, FailureKind::WindowSync)
            == FailurePolicy::Recoverable
        {
            self.record_frame_failure(FrameFailure::new(revision, FrameStage::WindowSync, &error));
            if let Some(native) = self.windows.current() {
                native.window().request_redraw();
            }
            Ok(())
        } else {
            Err(error)
        }
    }

    pub(super) fn sync_dom(&mut self, event_loop: &ActiveEventLoop) -> Result<(), HostError> {
        let (revision, desired) = {
            let state = self
                .dom_bindings
                .try_borrow()
                .map_err(|_| HostError::DomBorrowConflict)?;
            (state.dom.revision(), WindowSpec::from_dom(&state.dom)?)
        };
        if self.observed_revision == Some(revision) {
            return Ok(());
        }
        if self
            .pending_graphics_replacement
            .as_ref()
            .is_some_and(|pending| desired.as_ref() == Some(pending.prepared.spec()))
        {
            self.pending_graphics_replacement
                .as_mut()
                .expect("matching graphics replacement remains installed")
                .revision = revision;
            self.observed_revision = Some(revision);
            return Ok(());
        }
        self.cancel_graphics_replacement();

        // Record observation before native work so a rejected specification is
        // not retried on every event-loop turn.
        self.observed_revision = Some(revision);
        if desired.is_none() {
            self.queue_pointer_cancel()?;
        }
        let change = match self.windows.reconcile(event_loop, desired) {
            Ok(change) => change,
            Err(error) => {
                return self.handle_window_sync_failure(revision, error.into());
            }
        };

        match change {
            WindowChange::Created => {
                // Native creation ends the permitted initial windowless phase;
                // removing this final Window must exit even if GPU setup is
                // still pending.
                self.ever_had_window = true;
                // Ensure AppKit has applied the newly created native Window
                // before WGPU derives a presentation surface from it.
                event_loop.flush_windows();
                let native = self
                    .windows
                    .current()
                    .expect("a created window is installed before renderer creation");
                self.begin_graphics_initialization(
                    event_loop,
                    revision,
                    native.dom_id(),
                    Rc::clone(native.window()),
                );
            }
            WindowChange::PreparedReplacement(prepared) => {
                event_loop.flush_windows();
                if self.pending_graphics.is_some() {
                    debug_assert!(self.graphics.is_none());
                    debug_assert!(self.renderer.is_none());

                    // The candidate Window is ready, so the obsolete request can
                    // now be cancelled without risking loss of the active Window
                    // when native candidate creation fails.
                    self.cancel_graphics_initialization();
                    self.queue_pointer_cancel()?;
                    let previous_window = prepared.commit(&mut self.windows);
                    if let Some(previous_window) = previous_window {
                        previous_window.close();
                    }
                    let native = self
                        .windows
                        .current()
                        .expect("a committed replacement is installed before initialization");
                    self.begin_graphics_initialization(
                        event_loop,
                        revision,
                        native.dom_id(),
                        Rc::clone(native.window()),
                    );
                } else {
                    let graphics = self
                        .graphics
                        .as_ref()
                        .ok_or(HostError::MissingGraphicsContext)?;
                    let candidate_renderer = match classify_replacement_renderer(
                        WindowRenderer::new(graphics, Rc::clone(prepared.window())),
                    ) {
                        Ok(ReplacementRenderer::Reuse(renderer)) => renderer,
                        Ok(ReplacementRenderer::SelectCompatibleAdapter) => {
                            self.begin_graphics_replacement(event_loop, revision, prepared);
                            return Ok(());
                        }
                        Err(error) => {
                            // `prepared` closes only the candidate on return. The
                            // active Window, renderer, and presented plan remain
                            // installed when this is recoverable.
                            return self.handle_window_sync_failure(revision, error.into());
                        }
                    };

                    // No fallible work remains: install the already-created
                    // renderer with its candidate Window before releasing the old
                    // surface and closing the previous Window.
                    self.queue_pointer_cancel()?;
                    let (previous_window, previous_renderer) = prepared.commit_with(
                        &mut self.windows,
                        &mut self.renderer,
                        candidate_renderer,
                    );
                    // The retained plan belongs to the old Window and renderer.
                    // Never use it to classify the candidate's first-frame failure
                    // as recoverable or to hit-test the unpainted replacement.
                    self.discard_stale_presented_frame();
                    drop(previous_renderer);
                    if let Some(previous_window) = previous_window {
                        previous_window.close();
                    }
                    self.ever_had_window = true;
                }
            }
            WindowChange::Removed => {
                self.cancel_graphics_initialization();
                self.renderer = None;
                self.presented = None;
                self.dom_bindings.borrow_mut().clear_hover_path();
                self.cursor = None;
                if self.ever_had_window {
                    self.request_exit();
                }
            }
            WindowChange::Updated | WindowChange::Unchanged => {}
        }

        // A same-window size update can reconfigure the surface on the next
        // redraw. Stop exposing an old-size plan as soon as the native size no
        // longer matches the renderer's configured target.
        self.discard_stale_presented_frame();
        if self.renderer.is_some() {
            if let Some(native) = self.windows.current() {
                native.window().request_redraw();
            }
        }
        Ok(())
    }
}
