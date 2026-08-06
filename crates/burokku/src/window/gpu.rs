use std::{error::Error, sync::Arc};

use render::{wgpu, Canvas, RenderError, RenderTimings, Renderer, SurfaceSize, TextSystem};
use winit::{dpi::PhysicalSize, window::Window};

use crate::ui::{self, UiStore};

/// WebGPU state for the native window.
#[allow(clippy::upper_case_acronyms)]
pub struct GPU {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    canvas: Canvas,
    text_system: TextSystem,
    store: UiStore,
    ui_version: u64,
    // The instance must stay alive for as long as the surface is in use.
    _instance: wgpu::Instance,
}

impl GPU {
    pub async fn new(window: Arc<Window>, store: UiStore) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let renderer = Renderer::new(
            &instance,
            &surface,
            SurfaceSize::new(size.width, size.height),
        )
        .await?;

        let mut text_system = TextSystem::new();
        let (ui_version, snapshot) = store.snapshot_with_version();
        let canvas = build_canvas(&snapshot, size, window.scale_factor(), &mut text_system);

        Ok(Self {
            surface,
            renderer,
            canvas,
            text_system,
            store,
            ui_version,
            _instance: instance,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f64) {
        self.renderer
            .resize(&self.surface, SurfaceSize::new(size.width, size.height));
        let (version, snapshot) = self.store.snapshot_with_version();
        self.canvas = build_canvas(&snapshot, size, scale_factor, &mut self.text_system);
        self.ui_version = version;
    }

    pub fn sync_ui(&mut self, window: &Window) -> bool {
        let Some((version, snapshot)) = self.store.snapshot_if_changed(self.ui_version) else {
            return false;
        };
        self.canvas = build_canvas(
            &snapshot,
            window.inner_size(),
            window.scale_factor(),
            &mut self.text_system,
        );
        self.ui_version = version;
        true
    }

    pub fn render(&mut self, window: &Window) -> Result<RenderTimings, RenderError> {
        self.renderer.render_timed_with_pre_present(
            &self.surface,
            &self.canvas,
            &mut self.text_system,
            || window.pre_present_notify(),
        )
    }
}

fn build_canvas(
    root: &ui::Elements,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    text_system: &mut TextSystem,
) -> Canvas {
    let scale_factor = (scale_factor as f32).max(f32::EPSILON);
    ui::build_canvas(
        root,
        size.width as f32 / scale_factor,
        size.height as f32 / scale_factor,
        scale_factor,
        text_system,
    )
}
