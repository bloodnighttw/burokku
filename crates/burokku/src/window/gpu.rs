use std::{error::Error, sync::Arc};

use render::{wgpu, Canvas, RenderError, Renderer, SurfaceSize, TextSystem};
use winit::{dpi::PhysicalSize, window::Window};

use crate::dom::{self, DomStore};

/// The WebGPU state used by the application window.
#[allow(clippy::upper_case_acronyms)]
pub struct GPU {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    canvas: Canvas,
    text_system: TextSystem,
    dom: DomStore,
    dom_version: u64,
    // The instance must stay alive for as long as the surface is in use.
    _instance: wgpu::Instance,
}

impl GPU {
    pub async fn new(window: Arc<Window>, dom: DomStore) -> Result<Self, Box<dyn Error>> {
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
        let dom_version = dom.version();
        let canvas = build_canvas(&dom, size, window.scale_factor(), &mut text_system)?;

        Ok(Self {
            surface,
            renderer,
            canvas,
            text_system,
            dom,
            dom_version,
            _instance: instance,
        })
    }

    pub fn resize(
        &mut self,
        size: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<(), dom::DomRenderError> {
        self.renderer
            .resize(&self.surface, SurfaceSize::new(size.width, size.height));
        self.canvas = build_canvas(&self.dom, size, scale_factor, &mut self.text_system)?;
        Ok(())
    }

    pub fn sync_dom(&mut self, window: &Window) -> Result<bool, dom::DomRenderError> {
        let version = self.dom.version();
        if version == self.dom_version {
            return Ok(false);
        }
        self.canvas = build_canvas(
            &self.dom,
            window.inner_size(),
            window.scale_factor(),
            &mut self.text_system,
        )?;
        self.dom_version = version;
        Ok(true)
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

fn build_canvas(
    dom: &DomStore,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    text_system: &mut TextSystem,
) -> Result<Canvas, dom::DomRenderError> {
    let scale_factor = scale_factor as f32;
    dom::build_canvas(
        &dom.snapshot(),
        size.width as f32 / scale_factor,
        size.height as f32 / scale_factor,
        scale_factor,
        text_system,
    )
}
