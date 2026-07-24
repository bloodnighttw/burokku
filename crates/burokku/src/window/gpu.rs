use std::{error::Error, sync::Arc};

use render::{wgpu, RenderError, Renderer, SurfaceSize, TextSystem};
use winit::{dpi::PhysicalSize, window::Window};

use crate::ui::{self, Document, UiFrame, UiStore};

/// The WebGPU state used by the application window.
#[allow(clippy::upper_case_acronyms)]
pub struct GPU {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    frame: UiFrame,
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
        let frame = build_frame(&snapshot, size, window.scale_factor(), &mut text_system);

        Ok(Self {
            surface,
            renderer,
            frame,
            text_system,
            store,
            ui_version,
            _instance: instance,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f64) {
        self.renderer
            .resize(&self.surface, SurfaceSize::new(size.width, size.height));
        let (ui_version, snapshot) = self.store.snapshot_with_version();
        self.frame = build_frame(&snapshot, size, scale_factor, &mut self.text_system);
        self.ui_version = ui_version;
    }

    pub fn sync_ui(&mut self, window: &Window) -> bool {
        let Some((version, snapshot)) = self.store.snapshot_if_changed(self.ui_version) else {
            return false;
        };
        self.frame = build_frame(
            &snapshot,
            window.inner_size(),
            window.scale_factor(),
            &mut self.text_system,
        );
        self.ui_version = version;
        true
    }

    pub fn render(&mut self, window: &Window) -> Result<(), RenderError> {
        self.renderer.render_with_pre_present(
            &self.surface,
            &self.frame.canvas,
            &mut self.text_system,
            || window.pre_present_notify(),
        )
    }

    /// The current logical layout, retained for hit testing and input routing.
    #[allow(dead_code)]
    pub fn layout(&self) -> &ui::layouts::Layout {
        &self.frame.layout
    }
}

fn build_frame(
    document: &Document,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    text_system: &mut TextSystem,
) -> UiFrame {
    let scale_factor = scale_factor as f32;
    ui::build_frame(
        document,
        size.width as f32 / scale_factor,
        size.height as f32 / scale_factor,
        scale_factor,
        text_system,
    )
}
