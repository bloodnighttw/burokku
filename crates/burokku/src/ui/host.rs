//! Main-thread application handler that reconciles, lays out, paints, and presents.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::oneshot;
use winit::{
    application::ApplicationHandler, ActiveEventLoop, PhysicalSize, WindowEvent, WindowId,
};

use super::{
    elements::{NodeId, PublishedDom, PublishedDomReader},
    gpu::{GraphicsContext, GraphicsError, PresentationOutcome, WindowRenderer},
    layout::{LayoutEngine, LayoutError, LogicalViewport},
    scene::{BuiltScene, SceneError, ScenePlan},
    text::TextEngine,
    window_host::{WindowChange, WindowHostError, WindowManager, WindowSpec},
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingWindowStatus {
    Current,
    Removed,
    Replaced,
}

fn pending_window_status(
    pending_dom_id: NodeId,
    desired_dom_id: Option<NodeId>,
) -> PendingWindowStatus {
    match desired_dom_id {
        Some(desired_dom_id) if desired_dom_id == pending_dom_id => PendingWindowStatus::Current,
        Some(_) => PendingWindowStatus::Replaced,
        None => PendingWindowStatus::Removed,
    }
}

fn accept_current_graphics_result<T, E>(
    status: PendingWindowStatus,
    result: Result<T, E>,
) -> Option<Result<T, E>> {
    (status == PendingWindowStatus::Current).then_some(result)
}

fn graphics_initialization_stop_is_fatal(status: PendingWindowStatus) -> bool {
    status == PendingWindowStatus::Current
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PresentedSurface<W = WindowId> {
    window_id: W,
    physical_size: PhysicalSize<u32>,
    generation: u64,
}

#[derive(Debug)]
pub(crate) struct PresentedFrame<W = WindowId> {
    plan: ScenePlan,
    surface: PresentedSurface<W>,
}

impl<W> PresentedFrame<W> {
    pub(crate) fn revision(&self) -> u64 {
        self.plan.revision()
    }
}

fn presented_frame_is_usable<W: Eq>(
    frame: Option<&PresentedFrame<W>>,
    active_surface: Option<&PresentedSurface<W>>,
    last_presented_revision: Option<u64>,
) -> bool {
    frame.is_some_and(|frame| {
        active_surface == Some(&frame.surface) && last_presented_revision == Some(frame.revision())
    })
}

type GraphicsInitialization = Result<(GraphicsContext, WindowRenderer), GraphicsError>;

#[derive(Debug)]
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug)]
struct PendingGraphicsInitialization {
    revision: u64,
    dom_id: NodeId,
    window_id: WindowId,
    result: oneshot::Receiver<GraphicsInitialization>,
    _task: AbortOnDrop,
}

#[derive(Debug)]
pub(crate) struct ApplicationHost {
    publications: PublishedDomReader,
    latest_publication: Option<Arc<PublishedDom>>,
    graphics: Option<GraphicsContext>,
    pending_graphics: Option<PendingGraphicsInitialization>,
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
    pub(crate) fn new(publications: PublishedDomReader, text: TextEngine) -> Self {
        Self {
            publications,
            // Force `resumed` to reconcile the newest publication, whether it
            // contains a Window already or represents a valid windowless app.
            latest_publication: None,
            // GPU allocation is delayed until a native Window exists, so its
            // surface can constrain adapter selection.
            graphics: None,
            pending_graphics: None,
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

    fn discard_stale_presented_frame(&mut self) {
        if !self.has_usable_presented_frame() {
            self.presented = None;
            self.cursor_target = None;
        }
    }

    fn begin_graphics_initialization(
        &mut self,
        event_loop: &ActiveEventLoop,
        revision: u64,
        dom_id: NodeId,
        window: Arc<winit::Window>,
    ) {
        debug_assert!(self.graphics.is_none());
        debug_assert!(self.renderer.is_none());
        debug_assert!(self.pending_graphics.is_none());

        let window_id = window.id();
        let proxy = event_loop.create_proxy();
        let (sender, result) = oneshot::channel();
        let task = tokio::spawn(async move {
            let initialized = GraphicsContext::for_window(window).await;
            let _ = sender.send(initialized);
            proxy.wake_up();
        });
        self.pending_graphics = Some(PendingGraphicsInitialization {
            revision,
            dom_id,
            window_id,
            result,
            _task: AbortOnDrop(task),
        });
    }

    fn complete_graphics_initialization(&mut self) -> Result<(), HostError> {
        let Some(pending) = self.pending_graphics.as_mut() else {
            return Ok(());
        };

        let received = pending.result.try_recv();

        // A publication can arrive after the event-loop turn began. Leave an
        // obsolete request installed for `sync_publication` to cancel while it
        // commits the corresponding native removal or replacement. Successes
        // and failures use the same authority gate.
        let authoritative = self.publications.load();
        let desired_dom_id = WindowSpec::from_publication(&authoritative)?
            .as_ref()
            .map(WindowSpec::dom_id);
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

    fn cancel_graphics_initialization(&mut self) {
        drop(self.pending_graphics.take());
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
        self.cancel_graphics_initialization();
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
                    Arc::clone(native.window()),
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
                        Arc::clone(native.window()),
                    );
                } else {
                    let graphics = self
                        .graphics
                        .as_ref()
                        .ok_or(HostError::MissingGraphicsContext)?;
                    let candidate_renderer =
                        match WindowRenderer::new(graphics, Arc::clone(prepared.window())) {
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
                self.cursor_target = None;
                if self.ever_had_window {
                    event_loop.exit();
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

    fn redraw(&mut self) -> Result<PresentationOutcome, RedrawFailure> {
        let publication = Arc::clone(
            self.latest_publication
                .as_ref()
                .ok_or(RedrawFailure::Fatal(HostError::MissingPublication))?,
        );
        let revision = publication.revision();
        let native = self
            .windows
            .current()
            .ok_or(RedrawFailure::Fatal(HostError::MissingNativeWindow))?;
        let window_id = native.id();
        let physical_size = native.window().inner_size();
        let scale_factor = native.window().scale_factor();
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
        let has_presented_frame = presented_frame_is_usable(
            self.presented.as_ref(),
            active_surface.as_ref(),
            renderer.last_presented_revision(),
        );
        if !has_presented_frame {
            self.presented = None;
            self.cursor_target = None;
        }
        if let Err(error) = resize_result {
            return Err(classify_resize_failure(
                revision,
                has_presented_frame,
                error,
            ));
        }
        if physical_size.width == 0 || physical_size.height == 0 {
            return Ok(PresentationOutcome::Occluded);
        }
        let viewport = logical_viewport(physical_size, scale_factor).map_err(|error| {
            classify_fatal_failure(has_presented_frame, FailureKind::Invariant, error)
        })?;
        let computed = self
            .layout
            .compute(publication, viewport)
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
            self.cursor_target = None;
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

        // A ready renderer needs a host-owned frame after resume. First-window
        // initialization requests its redraw asynchronously on completion.
        if self.renderer.is_some() {
            if let Some(window) = self.windows.current() {
                window.window().request_redraw();
            }
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
                self.cancel_graphics_initialization();
                self.renderer = None;
                self.presented = None;
                self.cursor_target = None;
                self.windows.close();
                event_loop.exit();
            }
            WindowEvent::Resized(size)
            | WindowEvent::ScaleFactorChanged {
                new_inner_size: size,
                ..
            } => {
                if self.pending_graphics.is_some() {
                    return;
                }
                let Some(publication) = self.latest_publication.as_ref() else {
                    self.fail(event_loop, HostError::MissingPublication);
                    return;
                };
                let revision = publication.revision();
                let Some(graphics) = self.graphics.as_ref() else {
                    self.fail(event_loop, HostError::MissingGraphicsContext);
                    return;
                };
                let Some(renderer) = self.renderer.as_mut() else {
                    self.fail(event_loop, HostError::MissingRenderer);
                    return;
                };
                let resize_result = renderer.resize(graphics, size);
                let active_surface = (window_id == renderer.window_id()
                    && size == renderer.physical_size())
                .then_some(PresentedSurface {
                    window_id,
                    physical_size: renderer.physical_size(),
                    generation: renderer.surface_generation(),
                });
                let has_presented_frame = presented_frame_is_usable(
                    self.presented.as_ref(),
                    active_surface.as_ref(),
                    renderer.last_presented_revision(),
                );
                if !has_presented_frame {
                    self.presented = None;
                    self.cursor_target = None;
                }
                if let Err(error) = resize_result {
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
            WindowEvent::RedrawRequested if self.pending_graphics.is_some() => {}
            WindowEvent::RedrawRequested => match self.redraw() {
                Ok(PresentationOutcome::Reconfigure) => {
                    self.discard_stale_presented_frame();
                    if let Some(window) = self.windows.current() {
                        window.window().request_redraw();
                    }
                }
                Ok(PresentationOutcome::Timeout) => {
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
                self.discard_stale_presented_frame();
                self.cursor_target = self
                    .presented
                    .as_ref()
                    .and_then(|frame| frame.plan.hit_test_physical(position.x, position.y));
            }
            WindowEvent::MouseInput { position, .. } => {
                self.discard_stale_presented_frame();
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
        if let Err(error) = self
            .sync_publication(event_loop)
            .and_then(|()| self.complete_graphics_initialization())
            .and_then(|()| self.sync_publication(event_loop))
        {
            self.fail(event_loop, error);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.cancel_graphics_initialization();
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

fn finish_successful_presentation<W>(
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

    fn test_surface(
        window_id: u8,
        physical_size: PhysicalSize<u32>,
        generation: u64,
    ) -> PresentedSurface<u8> {
        PresentedSurface {
            window_id,
            physical_size,
            generation,
        }
    }

    #[test]
    fn windowless_host_does_not_initialize_graphics() {
        let dom = Dom::new();
        let (_publisher, publications) = DomPublisher::new(&dom, |_| {});
        let host = ApplicationHost::new(publications, TextEngine::without_system_fonts());

        assert!(host.graphics.is_none());
        assert!(host.pending_graphics.is_none());
        assert!(host.renderer.is_none());
    }

    #[test]
    fn pending_window_status_detects_removal_replacement_and_same_window_updates() {
        let mut dom = Dom::new();
        let window_a = dom.create_element(Element::from_tag(ElementTag::Window));
        let window_b = dom.create_element(Element::from_tag(ElementTag::Window));

        assert_eq!(
            pending_window_status(window_a, Some(window_a)),
            PendingWindowStatus::Current
        );
        assert_eq!(
            pending_window_status(window_a, None),
            PendingWindowStatus::Removed
        );
        assert_eq!(
            pending_window_status(window_a, Some(window_b)),
            PendingWindowStatus::Replaced
        );
    }

    #[test]
    fn stale_success_and_error_are_discarded_before_installation() {
        for status in [PendingWindowStatus::Removed, PendingWindowStatus::Replaced] {
            assert_eq!(
                accept_current_graphics_result(status, Ok::<_, &'static str>(7_u8)),
                None
            );
            assert_eq!(
                accept_current_graphics_result(status, Err::<u8, _>("stale error")),
                None
            );
        }

        assert_eq!(
            accept_current_graphics_result(
                PendingWindowStatus::Current,
                Ok::<_, &'static str>(7_u8)
            ),
            Some(Ok(7))
        );
        assert!(!graphics_initialization_stop_is_fatal(
            PendingWindowStatus::Removed
        ));
        assert!(!graphics_initialization_stop_is_fatal(
            PendingWindowStatus::Replaced
        ));
        assert!(graphics_initialization_stop_is_fatal(
            PendingWindowStatus::Current
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_stalled_graphics_initialization_aborts_its_task() {
        let task = tokio::spawn(std::future::pending());
        let abort = task.abort_handle();
        let task = AbortOnDrop(task);

        tokio::task::yield_now().await;
        drop(task);
        tokio::task::yield_now().await;

        assert!(abort.is_finished());
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
        finish_successful_presentation(
            &mut presented,
            &mut last_failure,
            first_plan,
            test_surface(1, PhysicalSize::new(320, 240), 1),
        );
        last_failure = Some(FrameFailure {
            revision: second_revision,
            stage: FrameStage::Scene,
            message: "injected scene failure".into(),
        });

        finish_successful_presentation(
            &mut presented,
            &mut last_failure,
            second_plan,
            test_surface(1, PhysicalSize::new(320, 240), 1),
        );

        assert_eq!(
            presented.as_ref().map(PresentedFrame::revision),
            Some(second_revision)
        );
        assert!(last_failure.is_none());
    }

    #[test]
    fn replacement_first_frame_failure_cannot_recover_from_the_old_window() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        dom.append_child(dom.root(), window).unwrap();
        let plan = scene_plan(&dom);
        let revision = plan.revision();
        let old_surface = test_surface(1, PhysicalSize::new(320, 240), 7);
        let replacement_surface = test_surface(2, PhysicalSize::new(320, 240), 8);
        let presented = PresentedFrame {
            plan,
            surface: old_surface,
        };

        assert!(presented_frame_is_usable(
            Some(&presented),
            Some(&old_surface),
            Some(revision),
        ));
        assert!(!presented_frame_is_usable(
            Some(&presented),
            Some(&replacement_surface),
            Some(revision),
        ));
        let has_presented_frame =
            presented_frame_is_usable(Some(&presented), Some(&replacement_surface), None);
        assert!(!has_presented_frame);
        assert!(matches!(
            classify_candidate_failure(
                revision + 1,
                has_presented_frame,
                FailureKind::Layout,
                FrameStage::Layout,
                HostError::MissingRenderer,
            ),
            RedrawFailure::Fatal(HostError::MissingRenderer)
        ));
    }

    #[test]
    fn resize_then_failure_cannot_recover_from_the_old_surface() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        dom.append_child(dom.root(), window).unwrap();
        let plan = scene_plan(&dom);
        let revision = plan.revision();
        let old_surface = test_surface(1, PhysicalSize::new(320, 240), 11);
        let resized_surface = test_surface(1, PhysicalSize::new(640, 480), 12);
        let presented = PresentedFrame {
            plan,
            surface: old_surface,
        };

        assert!(presented_frame_is_usable(
            Some(&presented),
            Some(&old_surface),
            Some(revision),
        ));
        assert!(!presented_frame_is_usable(
            Some(&presented),
            Some(&resized_surface),
            Some(revision),
        ));
        let has_presented_frame =
            presented_frame_is_usable(Some(&presented), Some(&resized_surface), None);
        assert!(!has_presented_frame);
        assert!(matches!(
            classify_candidate_failure(
                revision + 1,
                has_presented_frame,
                FailureKind::Scene,
                FrameStage::Scene,
                HostError::MissingRenderer,
            ),
            RedrawFailure::Fatal(HostError::MissingRenderer)
        ));
    }
}
