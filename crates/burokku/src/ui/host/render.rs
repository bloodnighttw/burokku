//! Host measurement, frame validity, scene construction, and presentation.

use std::rc::Rc;

use winit::{ActiveEventLoop, PhysicalSize, WindowId};

use super::{
    super::{
        gpu::{GraphicsError, PresentationOutcome},
        layout::{ComputedLayout, LogicalViewport},
        scene::{BuiltScene, ScenePlan},
    },
    ApplicationHost, HostError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FrameStage {
    WindowSync,
    Resize,
    Layout,
    Scene,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FrameFailure {
    pub(super) revision: u64,
    pub(super) stage: FrameStage,
    pub(super) message: String,
}

impl FrameFailure {
    pub(super) fn new(revision: u64, stage: FrameStage, error: &HostError) -> Self {
        Self {
            revision,
            stage,
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FailureKind {
    WindowSync,
    TargetTooLarge,
    Layout,
    Scene,
    ActivePresentation,
    Invariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FailurePolicy {
    Recoverable,
    Fatal,
}

pub(super) fn failure_policy(has_presented_frame: bool, kind: FailureKind) -> FailurePolicy {
    match kind {
        FailureKind::WindowSync
        | FailureKind::TargetTooLarge
        | FailureKind::Layout
        | FailureKind::Scene
            if has_presented_frame =>
        {
            FailurePolicy::Recoverable
        }
        FailureKind::WindowSync
        | FailureKind::TargetTooLarge
        | FailureKind::Layout
        | FailureKind::Scene
        | FailureKind::ActivePresentation
        | FailureKind::Invariant => FailurePolicy::Fatal,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PresentedSurface<W = WindowId> {
    pub(super) window_id: W,
    pub(super) physical_size: PhysicalSize<u32>,
    pub(super) generation: u64,
}

#[derive(Debug)]
pub(crate) struct PresentedFrame<W = WindowId> {
    pub(super) plan: ScenePlan,
    pub(super) surface: PresentedSurface<W>,
}

impl<W> PresentedFrame<W> {
    pub(crate) fn revision(&self) -> u64 {
        self.plan.revision()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PresentationState {
    pub(super) active_renderer_has_presented: bool,
    pub(super) usable_frame: bool,
}

pub(super) fn presentation_state<W: Eq>(
    frame: Option<&PresentedFrame<W>>,
    active_surface: Option<&PresentedSurface<W>>,
    last_presented_revision: Option<u64>,
) -> PresentationState {
    PresentationState {
        active_renderer_has_presented: last_presented_revision.is_some(),
        usable_frame: frame.is_some_and(|frame| {
            active_surface == Some(&frame.surface)
                && last_presented_revision == Some(frame.revision())
        }),
    }
}

pub(super) fn presented_frame_is_usable<W: Eq>(
    frame: Option<&PresentedFrame<W>>,
    active_surface: Option<&PresentedSurface<W>>,
    last_presented_revision: Option<u64>,
) -> bool {
    presentation_state(frame, active_surface, last_presented_revision).usable_frame
}

impl ApplicationHost {
    pub(super) fn has_usable_presented_frame(&self) -> bool {
        let (Some(native), Some(renderer)) = (self.windows.current(), self.renderer.as_ref())
        else {
            return false;
        };
        let native_size = native.window().inner_size();
        let active_surface = (native.id() == renderer.window_id()
            && native_size == renderer.physical_size())
        .then_some(PresentedSurface {
            window_id: renderer.window_id(),
            physical_size: renderer.physical_size(),
            generation: renderer.surface_generation(),
        });
        presented_frame_is_usable(
            self.presented.as_ref(),
            active_surface.as_ref(),
            renderer.last_presented_revision(),
        )
    }

    pub(super) fn discard_stale_presented_frame(&mut self) {
        let window = self.windows.current().map(|window| window.id());
        if self.hover_window != window {
            self.dom_bindings.borrow_mut().clear_hover_path();
            self.cursor = None;
            self.hover_window = window;
        }
        if !self.has_usable_presented_frame() {
            self.presented = None;
        }
    }

    pub(super) fn handle_redraw_failure(
        &mut self,
        event_loop: &ActiveEventLoop,
        failure: RedrawFailure,
    ) {
        match failure {
            RedrawFailure::Recoverable(failure) => self.record_frame_failure(failure),
            RedrawFailure::Fatal(error) => self.fail(event_loop, error),
        }
    }

    /// Measures the DOM without requiring a renderer or publishing presented geometry.
    /// Successful computations publish observer sizes through the layout engine.
    pub(super) fn measure_layout(
        &mut self,
        physical_size: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<Rc<ComputedLayout>, RedrawFailure> {
        let viewport =
            logical_viewport(physical_size, scale_factor).map_err(RedrawFailure::Fatal)?;
        let has_presented_frame = self.has_usable_presented_frame();
        let state = self
            .dom_bindings
            .try_borrow()
            .map_err(|_| RedrawFailure::Fatal(HostError::DomBorrowConflict))?;
        self.layout.compute(&state.dom, viewport).map_err(|error| {
            classify_candidate_failure(
                state.dom.revision(),
                has_presented_frame,
                FailureKind::Layout,
                FrameStage::Layout,
                error.into(),
            )
        })?;
        Ok(self
            .layout
            .current_shared()
            .expect("a successful layout computation installs current state"))
    }

    pub(super) fn redraw(&mut self) -> Result<PresentationOutcome, RedrawFailure> {
        let native = self
            .windows
            .current()
            .ok_or(RedrawFailure::Fatal(HostError::MissingNativeWindow))?;
        let window_id = native.id();
        let physical_size = native.window().inner_size();
        let scale_factor = native.window().scale_factor();
        // Measurement must survive zero-size surfaces and GPU resize/present failures.
        let computed = self.measure_layout(physical_size, scale_factor)?;
        let revision = computed.revision();
        let graphics = self
            .graphics
            .as_ref()
            .ok_or(RedrawFailure::Fatal(HostError::MissingGraphicsContext))?;
        let renderer = self
            .renderer
            .as_mut()
            .ok_or(RedrawFailure::Fatal(HostError::MissingRenderer))?;
        if window_id != renderer.window_id() {
            return Err(classify_fatal_failure(
                false,
                FailureKind::Invariant,
                HostError::WindowRendererMismatch,
            ));
        }

        let resize_result = renderer.resize(graphics, physical_size);
        let active_surface =
            (physical_size == renderer.physical_size()).then_some(PresentedSurface {
                window_id,
                physical_size: renderer.physical_size(),
                generation: renderer.surface_generation(),
            });
        let presentation = presentation_state(
            self.presented.as_ref(),
            active_surface.as_ref(),
            renderer.last_presented_revision(),
        );
        if !presentation.usable_frame {
            self.presented = None;
        }
        if let Err(error) = resize_result {
            return Err(classify_resize_failure(
                revision,
                presentation.active_renderer_has_presented,
                error,
            ));
        }
        let has_presented_frame = presentation.usable_frame;
        if physical_size.width == 0 || physical_size.height == 0 {
            return Ok(PresentationOutcome::Occluded);
        }
        let frame = {
            let state = self
                .dom_bindings
                .try_borrow()
                .map_err(|_| RedrawFailure::Fatal(HostError::DomBorrowConflict))?;
            let frame = BuiltScene::build(
                &state.dom,
                &computed,
                physical_size,
                scale_factor,
                renderer.resources_mut(),
            )
            .map_err(|error| {
                classify_candidate_failure(
                    revision,
                    has_presented_frame,
                    FailureKind::Scene,
                    FrameStage::Scene,
                    error.into(),
                )
            })?;
            debug_assert_eq!(state.dom.revision(), revision);
            frame
        };
        debug_assert!(frame.glyph_runs() <= frame.glyphs());
        let outcome = renderer.present(graphics, &frame).map_err(|error| {
            classify_fatal_failure(
                has_presented_frame,
                FailureKind::ActivePresentation,
                error.into(),
            )
        })?;
        if let PresentationOutcome::Presented {
            revision: presented_revision,
        } = outcome
        {
            debug_assert_eq!(presented_revision, revision);
            debug_assert_eq!(renderer.last_presented_revision(), Some(presented_revision));
            self.dom_bindings
                .try_borrow()
                .map_err(|_| RedrawFailure::Fatal(HostError::DomBorrowConflict))?
                .publish_presented_layout(computed);
            let surface = PresentedSurface {
                window_id,
                physical_size: renderer.physical_size(),
                generation: renderer.surface_generation(),
            };
            finish_successful_presentation(
                &mut self.presented,
                &mut self.last_frame_failure,
                frame.plan().clone(),
                surface,
            );
            // Recheck stationary pointers against newly presented geometry, not candidate layouts.
            self.queue_hover_at_cursor().map_err(RedrawFailure::Fatal)?;
        }
        Ok(outcome)
    }
}

pub(super) fn classify_candidate_failure(
    revision: u64,
    has_presented_frame: bool,
    kind: FailureKind,
    stage: FrameStage,
    error: HostError,
) -> RedrawFailure {
    if failure_policy(has_presented_frame, kind) == FailurePolicy::Recoverable {
        RedrawFailure::Recoverable(FrameFailure::new(revision, stage, &error))
    } else {
        RedrawFailure::Fatal(error)
    }
}

pub(super) fn classify_fatal_failure(
    has_presented_frame: bool,
    kind: FailureKind,
    error: HostError,
) -> RedrawFailure {
    debug_assert_eq!(
        failure_policy(has_presented_frame, kind),
        FailurePolicy::Fatal
    );
    RedrawFailure::Fatal(error)
}

pub(super) fn classify_resize_failure(
    revision: u64,
    active_renderer_has_presented: bool,
    error: GraphicsError,
) -> RedrawFailure {
    if matches!(&error, GraphicsError::TargetTooLarge { .. }) {
        classify_candidate_failure(
            revision,
            active_renderer_has_presented,
            FailureKind::TargetTooLarge,
            FrameStage::Resize,
            error.into(),
        )
    } else {
        RedrawFailure::Fatal(error.into())
    }
}

pub(super) fn finish_successful_presentation<W>(
    presented: &mut Option<PresentedFrame<W>>,
    last_frame_failure: &mut Option<FrameFailure>,
    plan: ScenePlan,
    surface: PresentedSurface<W>,
) {
    let revision = plan.revision();
    debug_assert_eq!(plan.physical_size(), surface.physical_size);
    *presented = Some(PresentedFrame { plan, surface });
    *last_frame_failure = None;
    debug_assert_eq!(
        presented.as_ref().map(PresentedFrame::revision),
        Some(revision)
    );
}

pub(super) fn logical_viewport(
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> Result<LogicalViewport, HostError> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return Err(HostError::InvalidScaleFactor(scale_factor));
    }
    let width = f64::from(physical_size.width) / scale_factor;
    let height = f64::from(physical_size.height) / scale_factor;
    if width > f64::from(f32::MAX) || height > f64::from(f32::MAX) {
        return Err(HostError::ViewportTooLarge { width, height });
    }
    Ok(LogicalViewport::new(width as f32, height as f32)?)
}

#[derive(Debug)]
pub(super) enum RedrawFailure {
    Recoverable(FrameFailure),
    Fatal(HostError),
}
