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

#[derive(Debug)]
pub(crate) struct PresentedFrame {
    plan: ScenePlan,
}

#[derive(Debug)]
pub(crate) struct ApplicationHost {
    publications: PublishedDomReader,
    publication: Option<Arc<PublishedDom>>,
    graphics: GraphicsContext,
    windows: WindowManager,
    renderer: Option<WindowRenderer>,
    layout: LayoutEngine<TextEngine>,
    presented: Option<PresentedFrame>,
    cursor_target: Option<NodeId>,
    ever_had_window: bool,
    fatal_error: Option<HostError>,
}

impl ApplicationHost {
    pub(crate) fn new(
        publications: PublishedDomReader,
        publication: Arc<PublishedDom>,
        graphics: GraphicsContext,
        windows: WindowManager,
        renderer: WindowRenderer,
        text: TextEngine,
    ) -> Self {
        Self {
            publications,
            publication: Some(publication),
            graphics,
            windows,
            renderer: Some(renderer),
            layout: LayoutEngine::new(text),
            presented: None,
            cursor_target: None,
            ever_had_window: true,
            fatal_error: None,
        }
    }

    pub(crate) fn fatal_error(&self) -> Option<&HostError> {
        self.fatal_error.as_ref()
    }

    fn warn_frame_failure(&self, error: &HostError) {
        debug_assert!(error.is_recoverable_frame_error());
        let revision = self
            .publication
            .as_ref()
            .map_or(0, |publication| publication.revision());
        eprintln!("Burokku warning: frame for DOM revision {revision} failed; continuing: {error}");
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
            .publication
            .as_ref()
            .is_some_and(|current| current.revision() == publication.revision())
        {
            return Ok(());
        }

        let change = self.windows.reconcile(event_loop, &publication)?;
        match change {
            WindowChange::Created => {
                let native = self
                    .windows
                    .current()
                    .expect("a created window is installed before renderer creation");
                self.renderer = Some(WindowRenderer::new(
                    &self.graphics,
                    Arc::clone(native.window()),
                )?);
                self.ever_had_window = true;
            }
            WindowChange::PreparedReplacement(prepared) => {
                let candidate_renderer = match WindowRenderer::new(
                    &self.graphics,
                    Arc::clone(prepared.window()),
                ) {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        // `prepared` closes only the candidate on return. Keep
                        // using the working native Window and renderer, accept
                        // the latest DOM for subsequent content frames, and do
                        // not retry this replacement on every event-loop turn.
                        eprintln!(
                            "Burokku warning: renderer setup for replacement Window at DOM revision {} failed; keeping the current Window: {error}",
                            publication.revision()
                        );
                        self.publication = Some(publication);
                        if let Some(native) = self.windows.current() {
                            native.window().request_redraw();
                        }
                        return Ok(());
                    }
                };

                // No fallible work remains: replace the native Window and its
                // renderer as one host transaction, then release the previous
                // surface before closing its Window.
                let previous_renderer = self.renderer.replace(candidate_renderer);
                let previous_window = prepared.commit(&mut self.windows);
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

        self.publication = Some(publication);
        if let Some(native) = self.windows.current() {
            native.window().request_redraw();
        }
        Ok(())
    }

    fn redraw(&mut self) -> Result<PresentationOutcome, HostError> {
        let publication = self
            .publication
            .as_ref()
            .ok_or(HostError::MissingPublication)?;
        let native = self
            .windows
            .current()
            .ok_or(HostError::MissingNativeWindow)?;
        let renderer = self.renderer.as_mut().ok_or(HostError::MissingRenderer)?;
        if native.id() != renderer.window_id() {
            return Err(HostError::WindowRendererMismatch);
        }

        let physical_size = native.window().inner_size();
        renderer.resize(&self.graphics, physical_size)?;
        if physical_size.width == 0 || physical_size.height == 0 {
            return Ok(PresentationOutcome::Occluded);
        }
        let scale_factor = native.window().scale_factor();
        let viewport = logical_viewport(physical_size, scale_factor)?;
        let computed = self.layout.compute(Arc::clone(publication), viewport)?;
        let frame = BuiltScene::build(
            computed,
            physical_size,
            scale_factor,
            renderer.resources_mut(),
        )?;
        debug_assert!(frame.glyph_runs() <= frame.glyphs());
        let outcome = renderer.present(&self.graphics, &frame)?;
        if let PresentationOutcome::Presented { revision } = outcome {
            debug_assert_eq!(revision, publication.revision());
            debug_assert_eq!(renderer.last_presented_revision(), Some(revision));
            self.presented = Some(PresentedFrame {
                plan: frame.plan().clone(),
            });
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

        // AppKit can perform the Window's first draw while GPU resources are
        // still being initialized, before run_app installs this handler. That
        // early RedrawRequested event is intentionally not retained by the
        // platform dispatcher, so always schedule the first host-owned frame
        // after the handler becomes active.
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
                let resize = self
                    .renderer
                    .as_mut()
                    .map(|renderer| renderer.resize(&self.graphics, size));
                if let Some(Err(error)) = resize {
                    self.fail(event_loop, error.into());
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
                Err(error) if error.is_recoverable_frame_error() => {
                    // Keep the current Window, renderer, and last presented
                    // frame. A later publication or native redraw may produce
                    // a valid frame; retrying immediately could busy-loop.
                    self.warn_frame_failure(&error);
                }
                Err(error) => self.fail(event_loop, error),
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

impl HostError {
    fn is_recoverable_frame_error(&self) -> bool {
        matches!(self, Self::Layout(_) | Self::Scene(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn layout_and_scene_failures_are_recoverable_frame_errors() {
        let layout = HostError::Layout(LayoutError::InvalidViewport {
            width: f32::NAN,
            height: 10.0,
        });
        let scene = HostError::Scene(SceneError::EmptyTarget);

        assert!(layout.is_recoverable_frame_error());
        assert!(scene.is_recoverable_frame_error());
    }

    #[test]
    fn host_and_graphics_failures_remain_fatal() {
        let missing_renderer = HostError::MissingRenderer;
        let surface_validation = HostError::Graphics(GraphicsError::SurfaceValidation);

        assert!(!missing_renderer.is_recoverable_frame_error());
        assert!(!surface_validation.is_recoverable_frame_error());
    }
}
