use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use thiserror::Error;
use tokio::sync::{oneshot, watch};
use vello_common::{
    kurbo::{Affine, Rect},
    peniko::Color,
};
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Resources, Scene, TextureBindings};
use wgpu::{
    CommandEncoderDescriptor, CurrentSurfaceTexture, Device, Instance, PresentMode, Queue, Surface,
    SurfaceConfiguration, TextureViewDescriptor,
};
use winit::{
    application::ApplicationHandler, ActiveEventLoop, ControlFlow, PhysicalSize, Window,
    WindowEvent, WindowId,
};

use super::{
    computed::ComputedState,
    elements::{DomSnapshot, Elements, SharedDom},
};

const DEFAULT_SCALE_FACTOR: f64 = 1.0;

/// Errors encountered while creating or presenting the MTS rendering state.
#[derive(Debug, Error)]
pub enum FrameError {
    #[error(transparent)]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error(transparent)]
    RequestAdapter(#[from] wgpu::RequestAdapterError),
    #[error(transparent)]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error(transparent)]
    Hybrid(#[from] vello_hybrid::RenderError),
    #[error("the physical render size {width}x{height} exceeds Vello Hybrid's u16 viewport")]
    ViewportTooLarge { width: u32, height: u32 },
    #[error(
        "computed data revision mismatch: DOM {dom_revision}, layout {layout_revision:?}, hit testing {hit_test_revision:?}"
    )]
    RevisionMismatch {
        dom_revision: u64,
        layout_revision: Option<u64>,
        hit_test_revision: Option<u64>,
    },
    #[error("the window surface was lost")]
    SurfaceLost,
    #[error("the window surface reported a validation error")]
    SurfaceValidation,
}

/// Result of one requested frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameOutcome {
    Presented(u64),
    Retry,
    Occluded,
}

/// Coalesces commit, resize, and native redraw requests into one pending frame.
#[derive(Debug, Default)]
struct FrameScheduler {
    redraw_pending: bool,
    presented_revision: Option<u64>,
}

impl FrameScheduler {
    fn request_redraw(&mut self) -> bool {
        if self.redraw_pending {
            return false;
        }
        self.redraw_pending = true;
        true
    }

    fn begin_frame(&mut self) -> bool {
        std::mem::take(&mut self.redraw_pending)
    }

    fn finish_frame(&mut self, revision: u64) {
        self.presented_revision = Some(revision);
    }

    fn cancel_pending(&mut self) {
        self.redraw_pending = false;
    }

    fn presented_revision(&self) -> Option<u64> {
        self.presented_revision
    }
}

/// A retained Vello scene tagged with the immutable DOM revision used to build
/// it. Scene construction and computed layout are both MTS-only operations.
struct SceneState {
    scene: Scene,
    source_revision: Option<u64>,
}

impl SceneState {
    fn new() -> Self {
        Self {
            scene: Scene::new(1, 1),
            source_revision: None,
        }
    }

    fn rebuild(
        &mut self,
        snapshot: &DomSnapshot,
        computed: &ComputedState,
        physical_size: PhysicalSize<u32>,
        logical_size: taffy::geometry::Size<f32>,
        scale_factor: f64,
    ) -> Result<(), FrameError> {
        let hit_test = computed.hit_test_data();
        let layout_revision = computed.source_revision();
        let hit_test_revision = hit_test.map(|data| data.source_revision());
        if layout_revision != Some(snapshot.revision())
            || hit_test_revision != Some(snapshot.revision())
        {
            return Err(FrameError::RevisionMismatch {
                dom_revision: snapshot.revision(),
                layout_revision,
                hit_test_revision,
            });
        }

        let scene_width =
            u16::try_from(physical_size.width).map_err(|_| FrameError::ViewportTooLarge {
                width: physical_size.width,
                height: physical_size.height,
            })?;
        let scene_height =
            u16::try_from(physical_size.height).map_err(|_| FrameError::ViewportTooLarge {
                width: physical_size.width,
                height: physical_size.height,
            })?;
        self.scene.reset_and_resize(scene_width, scene_height);
        self.scene.set_transform(Affine::scale(scale_factor));
        self.scene.set_paint(Color::from_rgb8(250, 250, 250));
        self.scene.fill_rect(&Rect::new(
            0.0,
            0.0,
            logical_size.width as f64,
            logical_size.height as f64,
        ));

        for entry in hit_test
            .expect("the revision check requires hit-test data")
            .entries()
        {
            if !(entry.size.width > 0.0 && entry.size.height > 0.0) {
                continue;
            }
            let Some(element) = snapshot.dom().element(entry.node) else {
                continue;
            };
            let Some(color) = snapshot
                .dom()
                .style(entry.node, "background-color")
                .and_then(|color| color.parse().ok())
                .or_else(|| element_color(element))
            else {
                continue;
            };
            let rect = Rect::new(
                entry.location.x as f64,
                entry.location.y as f64,
                (entry.location.x + entry.size.width) as f64,
                (entry.location.y + entry.size.height) as f64,
            );
            self.scene.set_paint(color);
            self.scene.fill_rect(&rect);
        }
        self.source_revision = Some(snapshot.revision());
        Ok(())
    }
}

impl std::fmt::Debug for SceneState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SceneState")
            .field("source_revision", &self.source_revision)
            .finish_non_exhaustive()
    }
}

fn element_color(element: &Elements) -> Option<Color> {
    match element {
        Elements::App | Elements::Window | Elements::_String { .. } => None,
        Elements::Div => Some(Color::from_rgb8(225, 228, 232)),
        Elements::Flex { .. } => Some(Color::from_rgb8(203, 224, 255)),
        Elements::Grid { .. } => Some(Color::from_rgb8(207, 238, 215)),
        Elements::Text => Some(Color::from_rgb8(45, 48, 54)),
    }
}

/// Window surface, Vello renderer, and computed state owned by MTS.
pub struct FrameRenderer {
    _instance: Instance,
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    device: Device,
    queue: Queue,
    renderer: Renderer,
    resources: Resources,
    computed: ComputedState,
    scene: SceneState,
}

impl FrameRenderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, FrameError> {
        let size = nonzero_size(window.inner_size());
        let instance = Instance::default();
        let surface = instance.create_surface(window)?;
        let adapter =
            wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&surface)).await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("burokku-vello-hybrid-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let surface_config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);
        let (renderer, resources) = Renderer::new(
            &device,
            &RenderTargetConfig {
                format,
                width: size.width,
                height: size.height,
            },
        );

        Ok(Self {
            _instance: instance,
            surface,
            surface_config,
            device,
            queue,
            renderer,
            resources,
            computed: ComputedState::new(),
            scene: SceneState::new(),
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.surface_config.width == size.width && self.surface_config.height == size.height {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn render_frame(
        &mut self,
        window: &Window,
        snapshot: &Arc<DomSnapshot>,
    ) -> Result<FrameOutcome, FrameError> {
        let physical_size = window.inner_size();
        if physical_size.width == 0 || physical_size.height == 0 {
            return Ok(FrameOutcome::Occluded);
        }
        self.resize(physical_size);

        let surface_texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => (texture, false),
            CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            CurrentSurfaceTexture::Timeout => return Ok(FrameOutcome::Retry),
            CurrentSurfaceTexture::Occluded => return Ok(FrameOutcome::Occluded),
            CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(FrameOutcome::Retry);
            }
            CurrentSurfaceTexture::Lost => return Err(FrameError::SurfaceLost),
            CurrentSurfaceTexture::Validation => return Err(FrameError::SurfaceValidation),
        };

        // CSS/Taffy coordinates are logical. Vello scales the complete scene to
        // the physical surface while layout remains independent of display DPI.
        let scale_factor = valid_scale_factor(window.scale_factor());
        let logical_width = physical_size.width as f32 / scale_factor as f32;
        let logical_height = physical_size.height as f32 / scale_factor as f32;
        let logical_size = taffy::geometry::Size {
            width: logical_width,
            height: logical_height,
        };
        self.computed.compute_layout(
            snapshot,
            taffy::geometry::Size {
                width: taffy::AvailableSpace::Definite(logical_size.width),
                height: taffy::AvailableSpace::Definite(logical_size.height),
            },
        );
        self.scene.rebuild(
            snapshot,
            &self.computed,
            physical_size,
            logical_size,
            scale_factor,
        )?;

        let frame = surface_texture.0;
        let frame_view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("burokku-vello-hybrid-render"),
            });
        self.renderer.render(
            &self.scene.scene,
            &mut self.resources,
            &self.device,
            &self.queue,
            &mut encoder,
            &RenderSize {
                width: physical_size.width,
                height: physical_size.height,
            },
            &frame_view,
            &TextureBindings::new(),
        )?;
        self.queue.submit([encoder.finish()]);
        window.pre_present_notify();
        frame.present();

        if surface_texture.1 {
            self.surface.configure(&self.device, &self.surface_config);
        }
        Ok(FrameOutcome::Presented(snapshot.revision()))
    }
}

impl std::fmt::Debug for FrameRenderer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FrameRenderer")
            .field("surface_config", &self.surface_config)
            .field("computed_revision", &self.computed.source_revision())
            .field("scene_revision", &self.scene.source_revision)
            .finish_non_exhaustive()
    }
}

/// Native event handler that owns all window, layout, scene, and GPU state on
/// MTS. BTS only publishes immutable snapshots and coalescing notifications.
pub struct UiApplication {
    window: Arc<Window>,
    shared_dom: SharedDom,
    commits: watch::Receiver<u64>,
    renderer: FrameRenderer,
    scheduler: FrameScheduler,
    occluded: bool,
    close_sender: Option<oneshot::Sender<()>>,
    external_exit: Arc<AtomicBool>,
    error: Option<FrameError>,
}

impl UiApplication {
    pub fn new(
        window: Arc<Window>,
        shared_dom: SharedDom,
        renderer: FrameRenderer,
        close_sender: oneshot::Sender<()>,
        external_exit: Arc<AtomicBool>,
    ) -> Self {
        let commits = shared_dom.subscribe();
        Self {
            window,
            shared_dom,
            commits,
            renderer,
            scheduler: FrameScheduler::default(),
            occluded: false,
            close_sender: Some(close_sender),
            external_exit,
            error: None,
        }
    }

    pub fn take_error(&mut self) -> Option<FrameError> {
        self.error.take()
    }

    fn schedule_redraw(&mut self) {
        if !self.occluded && self.scheduler.request_redraw() {
            self.window.request_redraw();
        }
    }

    fn consume_commit_notification(&mut self) {
        if !self.commits.has_changed().unwrap_or(false) {
            return;
        }
        let committed_revision = *self.commits.borrow_and_update();
        if self.scheduler.presented_revision() != Some(committed_revision) {
            self.schedule_redraw();
        }
    }

    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(sender) = self.close_sender.take() {
            let _ = sender.send(());
        }
        event_loop.exit();
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: FrameError) {
        self.error = Some(error);
        self.request_exit(event_loop);
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        if !self.scheduler.begin_frame() {
            return;
        }

        // This Arc is retained until scene construction, GPU submission, and
        // presentation have all completed. A concurrent BTS commit can only be
        // considered for a subsequent frame.
        let snapshot = self.shared_dom.load();
        match self.renderer.render_frame(&self.window, &snapshot) {
            Ok(FrameOutcome::Presented(revision)) => {
                self.scheduler.finish_frame(revision);
                self.consume_commit_notification();
            }
            Ok(FrameOutcome::Retry) => self.schedule_redraw(),
            Ok(FrameOutcome::Occluded) => self.occluded = true,
            Err(error) => self.fail(event_loop, error),
        }
    }
}

impl ApplicationHandler for UiApplication {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        self.schedule_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if window_id != self.window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => self.request_exit(event_loop),
            WindowEvent::Resized(size) => {
                self.renderer.resize(size);
                self.schedule_redraw();
            }
            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                self.renderer.resize(new_inner_size);
                self.schedule_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if occluded {
                    self.scheduler.cancel_pending();
                } else {
                    self.schedule_redraw();
                }
            }
            WindowEvent::Focused(_)
            | WindowEvent::KeyboardInput(_)
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. } => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.external_exit.load(Ordering::Acquire) {
            self.request_exit(event_loop);
            return;
        }
        self.consume_commit_notification();
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(sender) = self.close_sender.take() {
            let _ = sender.send(());
        }
    }
}

fn valid_scale_factor(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        DEFAULT_SCALE_FACTOR
    }
}

fn nonzero_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::{BtsDom, Elements};

    #[test]
    fn scheduler_coalesces_requests_and_tracks_presented_revision() {
        let mut scheduler = FrameScheduler::default();

        assert!(scheduler.request_redraw());
        assert!(!scheduler.request_redraw());
        assert!(scheduler.begin_frame());
        assert!(!scheduler.begin_frame());
        scheduler.finish_frame(7);
        assert_eq!(scheduler.presented_revision(), Some(7));
        assert!(scheduler.request_redraw());
        scheduler.cancel_pending();
        assert!(scheduler.request_redraw());
    }

    #[test]
    fn scene_is_tagged_with_the_same_complete_revision_as_layout() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window);
            let div = dom.create(Elements::Div);
            dom.append_child(root, window).unwrap();
            dom.append_child(window, div).unwrap();
        }
        owner.checkpoint().unwrap();
        let snapshot = shared.load();
        let mut computed = ComputedState::new();
        computed.compute_layout(
            &snapshot,
            taffy::geometry::Size {
                width: taffy::AvailableSpace::Definite(800.0),
                height: taffy::AvailableSpace::Definite(600.0),
            },
        );
        let mut scene = SceneState::new();
        let physical_size = PhysicalSize::new(1600, 1200);
        let logical_size = taffy::geometry::Size {
            width: 800.0,
            height: 600.0,
        };

        scene
            .rebuild(&snapshot, &computed, physical_size, logical_size, 2.0)
            .unwrap();

        assert_eq!(scene.source_revision, Some(snapshot.revision()));
        assert_eq!(
            computed.hit_test_data().unwrap().source_revision(),
            snapshot.revision()
        );

        owner.mutate().create(Elements::Div);
        owner.checkpoint().unwrap();
        let newer = shared.load();
        assert!(matches!(
            scene.rebuild(&newer, &computed, physical_size, logical_size, 2.0),
            Err(FrameError::RevisionMismatch { .. })
        ));
        assert_eq!(
            scene.source_revision,
            Some(snapshot.revision()),
            "a mismatched snapshot must not replace the last coherent scene"
        );
    }

    #[test]
    fn invalid_scale_factors_fall_back_to_one() {
        assert_eq!(valid_scale_factor(2.0), 2.0);
        assert_eq!(valid_scale_factor(0.0), 1.0);
        assert_eq!(valid_scale_factor(f64::NAN), 1.0);
    }
}
