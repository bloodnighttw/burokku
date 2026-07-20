use std::{error::Error, sync::Arc};

use render::{wgpu, Canvas, Color, RenderError, Renderer, SurfaceSize, TextSystem};
use winit::{dpi::PhysicalSize, window::Window};

/// The WebGPU state used by the application window.
#[allow(clippy::upper_case_acronyms)]
pub struct GPU {
    // The instance must stay alive for as long as the surface is in use.
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    canvas: Canvas,
    text_system: TextSystem,
}

impl GPU {
    pub fn new(window: Arc<Window>) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let renderer = pollster::block_on(Renderer::new(
            &instance,
            &surface,
            SurfaceSize::new(size.width, size.height),
        ))?;

        Ok(Self {
            _instance: instance,
            surface,
            renderer,
            canvas: Canvas::new().with_clear_color(Color::WHITE),
            text_system: TextSystem::new(),
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        self.renderer
            .resize(&self.surface, SurfaceSize::new(size.width, size.height));
    }

    pub fn render(&mut self, window: &Window) -> Result<(), RenderError> {
        self.renderer.render_with_pre_present(
            &self.surface,
            &self.canvas,
            &mut self.text_system,
            || window.pre_present_notify(),
        )
    }
}
