//! Main-thread application handler that reconciles, lays out, paints, and presents.

use std::sync::Arc;

use thiserror::Error;
use winit::{
    application::ApplicationHandler, ActiveEventLoop, PhysicalSize, WindowEvent, WindowId,
};

use super::{
    elements::{NodeId, PublishedDom, PublishedDomReader},
    gpu::{GraphicsContext, GraphicsError, PresentationOutcome, WindowRenderer},
    layout::{LayoutEngine, LayoutError, LogicalViewport},
    scene::{BuiltScene, SceneError, ScenePlan},
    text::TextEngine,
    window_host::{WindowChange, WindowHostError, WindowManager},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameStage {
    WindowSync,
    Resize,
    Layout,
    Scene,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FrameFailure {
    revision: u64,
    stage: FrameStage,
    message: String,
}

impl FrameFailure {
    fn new(revision: u64, stage: FrameStage, error: &HostError) -> Self {
        Self {
            revision,
            stage,
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureKind {
    WindowSync,
    TargetTooLarge,
    Layout,
    Scene,
    ActivePresentation,
    Invariant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailurePolicy {
    Recoverable,
    Fatal,
}

fn failure_policy(has_presented_frame: bool, kind: FailureKind) -> FailurePolicy {
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

#[derive(Debug)]
pub(crate) struct PresentedFrame {
    plan: ScenePlan,
}

impl PresentedFrame {
    pub(crate) fn revision(&self) -> u64 {
        self.plan.revision()
    }
}

#[derive(Debug)]
pub(crate) struct ApplicationHost {
    publications: PublishedDomReader,
    latest_publication: Option<Arc<PublishedDom>>,
    graphics: GraphicsContext,
    windows: WindowManager,
    renderer: Option<WindowRenderer>,
    layout: LayoutEngine<TextEngine>,
    presented: Option<PresentedFrame>,
    last_frame_failure: Option<FrameFailure>,
    cursor_target: Option<NodeId>,
    ever_had_window: bool,
    fatal_error: Option<HostError>,
}

impl ApplicationHost {
    pub(crate) fn new(
        publications: PublishedDomReader,
        graphics: GraphicsContext,
        text: TextEngine,
    ) -> Self {
        Self {
            publications,
            // Force `resumed` to reconcile the newest publication, whether it
            // contains a Window already or represents a valid windowless app.
            latest_publication: None,
            graphics,
            windows: WindowManager::default(),
            renderer: None,
            layout: LayoutEngine::new(text),
            presented: None,
            last_frame_failure: None,
            cursor_target: None,
            ever_had_window: false,
            fatal_error: None,
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

    fn has_usable_presented_frame(&self) -> bool {
        if self.presented.is_none() {
            return false;
        }
        match (self.windows.current(), self.renderer.as_ref()) {
            (Some(native), Some(renderer)) => native.id() == renderer.window_id(),
            (None, _) | (_, None) => false,
        }
    }

    fn handle_window_sync_failure(
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

    fn handle_redraw_failure(&mut self, event_loop: &ActiveEventLoop, failure: RedrawFailure) {
        match failure {
            RedrawFailure::Recoverable(failure) => self.record_frame_failure(failure),
            RedrawFailure::Fatal(error) => self.fail(event_loop, error),
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: HostError) {
        if self.fatal_error.is_none() {
            self.fatal_error = Some(error);
        }
        self.renderer = None;
        self.windows.close();
        event_loop.exit();
    }

    fn sync_publication(&mut self, event_loop: &ActiveEventLoop) -> Result<(), HostError> {
        let publication = self.publications.load();
        if self
            .latest_publication
            .as_ref()
            .is_some_and(|current| current.revision() == publication.revision())
        {
            return Ok(());
        }

        // Observation and native-window application are separate. Recording
        // the publication first prevents a failed WindowSpec from being
        // retried on every event-loop turn and keeps the latest DOM as the
        // content target for the existing native viewport.
        let revision = publication.revision();
        self.latest_publication = Some(Arc::clone(&publication));
        let change = match self.windows.reconcile(event_loop, &publication) {
            Ok(change) => change,
            Err(error) => {
                return self.handle_window_sync_failure(revision, error.into());
            }
        };

        match change {
            WindowChange::Created => {
                // Ensure AppKit has applied the newly created native Window
                // before WGPU derives a presentation surface from it.
                event_loop.flush_windows();
                let native = self
                    .windows
                    .current()
                    .expect("a created window is installed before renderer creation");
                let renderer =
                    match WindowRenderer::new(&self.graphics, Arc::clone(native.window())) {
                        Ok(renderer) => renderer,
                        Err(error) => {
                            return self.handle_window_sync_failure(revision, error.into());
                        }
                    };
                self.renderer = Some(renderer);
                self.ever_had_window = true;
            }
            WindowChange::PreparedReplacement(prepared) => {
                event_loop.flush_windows();
                let candidate_renderer =
                    match WindowRenderer::new(&self.graphics, Arc::clone(prepared.window())) {
                        Ok(renderer) => renderer,
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
                let (previous_window, previous_renderer) =
                    prepared.commit_with(&mut self.windows, &mut self.renderer, candidate_renderer);
                drop(previous_renderer);
                if let Some(previous_window) = previous_window {
                    previous_window.close();
                }
                self.ever_had_window = true;
            }
            WindowChange::Removed => {
                self.renderer = None;
                self.presented = None;
                if self.ever_had_window {
                    event_loop.exit();
                }
            }
            WindowChange::Updated | WindowChange::Unchanged => {}
        }

        if let Some(native) = self.windows.current() {
            native.window().request_redraw();
        }
        Ok(())
    }

    fn redraw(&mut self) -> Result<PresentationOutcome, RedrawFailure> {
        let publication = self
            .latest_publication
            .as_ref()
            .ok_or(RedrawFailure::Fatal(HostError::MissingPublication))?;
        let revision = publication.revision();
        let has_presented_frame = self.presented.is_some();
        let native = self
            .windows
            .current()
            .ok_or(RedrawFailure::Fatal(HostError::MissingNativeWindow))?;
        let renderer = self
            .renderer
            .as_mut()
            .ok_or(RedrawFailure::Fatal(HostError::MissingRenderer))?;
        if native.id() != renderer.window_id() {
            return Err(classify_fatal_failure(
                has_presented_frame,
                FailureKind::Invariant,
                HostError::WindowRendererMismatch,
            ));
        }

        let physical_size = native.window().inner_size();
        if let Err(error) = renderer.resize(&self.graphics, physical_size) {
            return Err(classify_resize_failure(
                revision,
                has_presented_frame,
                error,
            ));
        }
        if physical_size.width == 0 || physical_size.height == 0 {
            return Ok(PresentationOutcome::Occluded);
        }
        let scale_factor = native.window().scale_factor();
        let viewport = logical_viewport(physical_size, scale_factor).map_err(|error| {
            classify_fatal_failure(has_presented_frame, FailureKind::Invariant, error)
        })?;
        let computed = self
            .layout
            .compute(Arc::clone(publication), viewport)
            .map_err(|error| {
                classify_candidate_failure(
                    revision,
                    has_presented_frame,
                    FailureKind::Layout,
                    FrameStage::Layout,
                    error.into(),
                )
            })?;
        let frame = BuiltScene::build(
            computed,
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
        debug_assert!(frame.glyph_runs() <= frame.glyphs());
        let outcome = renderer.present(&self.graphics, &frame).map_err(|error| {
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
            finish_successful_presentation(
                &mut self.presented,
                &mut self.last_frame_failure,
                frame.plan().clone(),
            );
        }
        Ok(outcome)
    }
}

impl ApplicationHandler for ApplicationHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.sync_publication(event_loop) {
            self.fail(event_loop, error);
            return;
        }

        // Native creation and surface setup can produce redraw demand before
        // reconciliation has installed every renderer resource. Always
        // schedule one host-owned frame after the handler becomes active.
        if let Some(window) = self.windows.current() {
            window.window().request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .windows
            .current()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.renderer = None;
                self.windows.close();
                event_loop.exit();
            }
            WindowEvent::Resized(size)
            | WindowEvent::ScaleFactorChanged {
                new_inner_size: size,
                ..
            } => {
                let Some(publication) = self.latest_publication.as_ref() else {
                    self.fail(event_loop, HostError::MissingPublication);
                    return;
                };
                let revision = publication.revision();
                let has_presented_frame = self.presented.is_some();
                let Some(renderer) = self.renderer.as_mut() else {
                    self.fail(event_loop, HostError::MissingRenderer);
                    return;
                };
                if let Err(error) = renderer.resize(&self.graphics, size) {
                    let failure = classify_resize_failure(revision, has_presented_frame, error);
                    self.handle_redraw_failure(event_loop, failure);
                    return;
                }
                if size.width > 0 && size.height > 0 {
                    if let Some(window) = self.windows.current() {
                        window.window().request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => match self.redraw() {
                Ok(PresentationOutcome::Timeout | PresentationOutcome::Reconfigure) => {
                    if let Some(window) = self.windows.current() {
                        window.window().request_redraw();
                    }
                }
                Ok(PresentationOutcome::Presented { .. } | PresentationOutcome::Occluded) => {}
                Err(failure) => {
                    // Recoverable candidate failures retain the Window,
                    // renderer, and presented hit-test plan. Do not request an
                    // immediate retry for the unchanged revision/viewport.
                    self.handle_redraw_failure(event_loop, failure);
                }
            },
            WindowEvent::CursorMoved { position } => {
                self.cursor_target = self
                    .presented
                    .as_ref()
                    .and_then(|frame| frame.plan.hit_test_physical(position.x, position.y));
            }
            WindowEvent::MouseInput { position, .. } => {
                self.cursor_target = self
                    .presented
                    .as_ref()
                    .and_then(|frame| frame.plan.hit_test_physical(position.x, position.y));
                let _target_for_problem_9_dispatch = self.cursor_target;
            }
            WindowEvent::Occluded(false) => {
                // On macOS, WGPU can reject the first surface texture as
                // occluded while AppKit is still making a newly shown window
                // visible. Retry once AppKit confirms visibility; otherwise
                // the next redraw may not arrive until the user resizes.
                if let Some(window) = self.windows.current() {
                    window.window().request_redraw();
                }
            }
            WindowEvent::Focused(_)
            | WindowEvent::Occluded(true)
            | WindowEvent::KeyboardInput(_)
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::MouseWheel { .. } => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.fatal_error.is_some() {
            event_loop.exit();
            return;
        }
        if let Err(error) = self.sync_publication(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.renderer = None;
        self.windows.close();
    }
}

fn classify_candidate_failure(
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

fn classify_fatal_failure(
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

fn classify_resize_failure(
    revision: u64,
    has_presented_frame: bool,
    error: GraphicsError,
) -> RedrawFailure {
    if matches!(&error, GraphicsError::TargetTooLarge { .. }) {
        classify_candidate_failure(
            revision,
            has_presented_frame,
            FailureKind::TargetTooLarge,
            FrameStage::Resize,
            error.into(),
        )
    } else {
        RedrawFailure::Fatal(error.into())
    }
}

fn finish_successful_presentation(
    presented: &mut Option<PresentedFrame>,
    last_frame_failure: &mut Option<FrameFailure>,
    plan: ScenePlan,
) {
    let revision = plan.revision();
    *presented = Some(PresentedFrame { plan });
    *last_frame_failure = None;
    debug_assert_eq!(
        presented.as_ref().map(PresentedFrame::revision),
        Some(revision)
    );
}

fn logical_viewport(
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
enum RedrawFailure {
    Recoverable(FrameFailure),
    Fatal(HostError),
}

#[derive(Debug, Error)]
pub(crate) enum HostError {
    #[error("no committed DOM publication is available")]
    MissingPublication,

    #[error("the committed Window has no native window")]
    MissingNativeWindow,

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
mod tests {
    use crate::ui::elements::{Dom, DomPublisher, Element, ElementTag};

    use super::*;

    fn scene_plan(dom: &Dom) -> ScenePlan {
        let (_publisher, reader) = DomPublisher::new(dom, |_| {});
        let mut layout = LayoutEngine::new(TextEngine::without_system_fonts());
        let computed = layout
            .compute(reader.load(), LogicalViewport::new(320.0, 240.0).unwrap())
            .unwrap();
        ScenePlan::from_layout(computed, PhysicalSize::new(320, 240), 1.0).unwrap()
    }

    fn oversized_target() -> GraphicsError {
        GraphicsError::TargetTooLarge {
            size: PhysicalSize::new(70_000, 10),
            max_texture_dimension_2d: 16_384,
            max_vello_dimension: 65_535,
        }
    }

    #[test]
    fn physical_pixels_convert_to_logical_viewport_once() {
        let viewport = logical_viewport(PhysicalSize::new(1600, 1200), 2.0).unwrap();
        assert_eq!(viewport.width(), 800.0);
        assert_eq!(viewport.height(), 600.0);
    }

    #[test]
    fn invalid_scale_factors_are_rejected() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                logical_viewport(PhysicalSize::new(800, 600), scale),
                Err(HostError::InvalidScaleFactor(_))
            ));
        }
    }

    #[test]
    fn candidate_failures_recover_only_after_a_frame_was_presented() {
        for kind in [
            FailureKind::WindowSync,
            FailureKind::TargetTooLarge,
            FailureKind::Layout,
            FailureKind::Scene,
        ] {
            assert_eq!(failure_policy(false, kind), FailurePolicy::Fatal);
            assert_eq!(failure_policy(true, kind), FailurePolicy::Recoverable);
        }
    }

    #[test]
    fn active_presentation_and_invariant_failures_remain_fatal() {
        for kind in [FailureKind::ActivePresentation, FailureKind::Invariant] {
            assert_eq!(failure_policy(false, kind), FailurePolicy::Fatal);
            assert_eq!(failure_policy(true, kind), FailurePolicy::Fatal);
        }

        assert!(matches!(
            classify_fatal_failure(
                true,
                FailureKind::ActivePresentation,
                HostError::Graphics(GraphicsError::SurfaceValidation),
            ),
            RedrawFailure::Fatal(HostError::Graphics(GraphicsError::SurfaceValidation))
        ));
    }

    #[test]
    fn oversized_resize_is_recoverable_only_with_a_presented_frame() {
        assert!(matches!(
            classify_resize_failure(42, false, oversized_target()),
            RedrawFailure::Fatal(HostError::Graphics(GraphicsError::TargetTooLarge { .. }))
        ));

        let failure = classify_resize_failure(42, true, oversized_target());
        assert!(matches!(
            failure,
            RedrawFailure::Recoverable(FrameFailure {
                revision: 42,
                stage: FrameStage::Resize,
                ..
            })
        ));
    }

    #[test]
    fn successful_presentation_advances_the_plan_and_clears_failure() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        dom.append_child(dom.root(), window).unwrap();
        let first_plan = scene_plan(&dom);
        let first_revision = first_plan.revision();

        dom.set_attribute(window, "title".into(), "updated".into())
            .unwrap();
        let second_plan = scene_plan(&dom);
        let second_revision = second_plan.revision();
        assert!(second_revision > first_revision);

        let mut presented = None;
        let mut last_failure = None;
        finish_successful_presentation(&mut presented, &mut last_failure, first_plan);
        last_failure = Some(FrameFailure {
            revision: second_revision,
            stage: FrameStage::Scene,
            message: "injected scene failure".into(),
        });

        finish_successful_presentation(&mut presented, &mut last_failure, second_plan);

        assert_eq!(
            presented.as_ref().map(PresentedFrame::revision),
            Some(second_revision)
        );
        assert!(last_failure.is_none());
    }
}
