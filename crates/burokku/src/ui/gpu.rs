//! WGPU surface and Vello Hybrid presentation ownership.

use std::sync::Arc;

use thiserror::Error;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Resources, TextureBindings};
use wgpu::{
    Adapter, CurrentSurfaceTexture, Device, Instance, Queue, Surface, SurfaceConfiguration,
    TextureFormat,
};
use winit::{PhysicalSize, Window, WindowId};

use super::scene::BuiltScene;

#[derive(Debug)]
pub(crate) struct GraphicsContext {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
}

impl GraphicsContext {
    pub(crate) async fn for_window(window: Arc<Window>) -> Result<Self, GraphicsError> {
        let instance = Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|error| GraphicsError::Surface(error.to_string()))?;
        let graphics = Self::request(instance.clone(), Some(&surface)).await?;
        drop(surface);
        Ok(graphics)
    }

    async fn request(
        instance: Instance,
        compatible_surface: Option<&Surface<'_>>,
    ) -> Result<Self, GraphicsError> {
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface,
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(primary) => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface,
                })
                .await
                .map_err(|fallback| {
                    GraphicsError::Adapter(format!(
                        "primary selection failed ({primary}); fallback selection failed ({fallback})"
                    ))
                })?,
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Burokku device"),
                required_features: wgpu::Features::empty(),
                ..Default::default()
            })
            .await
            .map_err(|error| GraphicsError::Device(error.to_string()))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresentationOutcome {
    Presented { revision: u64 },
    Timeout,
    Occluded,
    Reconfigure,
}

#[derive(Debug)]
pub(crate) struct WindowRenderer {
    window: Arc<Window>,
    surface: Surface<'static>,
    config: SurfaceConfiguration,
    renderer: Renderer,
    resources: Resources,
    suspended: bool,
    last_presented_revision: Option<u64>,
}

impl WindowRenderer {
    pub(crate) fn new(
        graphics: &GraphicsContext,
        window: Arc<Window>,
    ) -> Result<Self, GraphicsError> {
        let surface = graphics
            .instance
            .create_surface(window.clone())
            .map_err(|error| GraphicsError::Surface(error.to_string()))?;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Err(GraphicsError::EmptySurface);
        }
        let capabilities = surface.get_capabilities(&graphics.adapter);
        let format = choose_surface_format(&capabilities.formats)
            .ok_or(GraphicsError::UnsupportedSurface)?;
        let mut config = surface
            .get_default_config(&graphics.adapter, size.width, size.height)
            .ok_or(GraphicsError::UnsupportedSurface)?;
        config.format = format;
        surface.configure(&graphics.device, &config);
        let (renderer, resources) = Renderer::new(
            &graphics.device,
            &RenderTargetConfig {
                format,
                width: size.width,
                height: size.height,
            },
        );

        Ok(Self {
            window,
            surface,
            config,
            renderer,
            resources,
            suspended: false,
            last_presented_revision: None,
        })
    }

    pub(crate) fn window_id(&self) -> WindowId {
        self.window.id()
    }

    pub(crate) fn physical_size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(self.config.width, self.config.height)
    }

    pub(crate) fn resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }

    pub(crate) fn last_presented_revision(&self) -> Option<u64> {
        self.last_presented_revision
    }

    pub(crate) fn resize(&mut self, graphics: &GraphicsContext, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            self.suspended = true;
            return;
        }
        if !self.suspended && self.config.width == size.width && self.config.height == size.height {
            return;
        }

        self.suspended = false;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&graphics.device, &self.config);
        let (renderer, resources) = Renderer::new(
            &graphics.device,
            &RenderTargetConfig {
                format: self.config.format,
                width: size.width,
                height: size.height,
            },
        );
        self.renderer = renderer;
        self.resources = resources;
        self.last_presented_revision = None;
    }

    pub(crate) fn present(
        &mut self,
        graphics: &GraphicsContext,
        frame: &BuiltScene,
    ) -> Result<PresentationOutcome, GraphicsError> {
        if self.suspended {
            return Ok(PresentationOutcome::Occluded);
        }
        if frame.plan().physical_size() != self.physical_size() {
            return Err(GraphicsError::FrameSizeMismatch {
                frame: frame.plan().physical_size(),
                surface: self.physical_size(),
            });
        }

        let (surface_texture, suboptimal) = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => (texture, false),
            CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            CurrentSurfaceTexture::Timeout => return Ok(PresentationOutcome::Timeout),
            CurrentSurfaceTexture::Occluded => return Ok(PresentationOutcome::Occluded),
            CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&graphics.device, &self.config);
                return Ok(PresentationOutcome::Reconfigure);
            }
            CurrentSurfaceTexture::Lost => {
                self.recreate_surface(graphics)?;
                return Ok(PresentationOutcome::Reconfigure);
            }
            CurrentSurfaceTexture::Validation => return Err(GraphicsError::SurfaceValidation),
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = graphics
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Burokku frame"),
            });
        self.renderer
            .render(
                frame.scene(),
                &mut self.resources,
                &graphics.device,
                &graphics.queue,
                &mut encoder,
                &RenderSize {
                    width: self.config.width,
                    height: self.config.height,
                },
                &view,
                &TextureBindings::new(),
            )
            .map_err(|error| GraphicsError::Render(error.to_string()))?;
        graphics.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
        self.last_presented_revision = Some(frame.plan().revision());

        if suboptimal {
            self.surface.configure(&graphics.device, &self.config);
        }
        Ok(PresentationOutcome::Presented {
            revision: frame.plan().revision(),
        })
    }

    fn recreate_surface(&mut self, graphics: &GraphicsContext) -> Result<(), GraphicsError> {
        let surface = graphics
            .instance
            .create_surface(Arc::clone(&self.window))
            .map_err(|error| GraphicsError::Surface(error.to_string()))?;
        let capabilities = surface.get_capabilities(&graphics.adapter);
        let format = choose_surface_format(&capabilities.formats)
            .ok_or(GraphicsError::UnsupportedSurface)?;
        self.config.format = format;
        surface.configure(&graphics.device, &self.config);
        let (renderer, resources) = Renderer::new(
            &graphics.device,
            &RenderTargetConfig {
                format,
                width: self.config.width,
                height: self.config.height,
            },
        );
        self.surface = surface;
        self.renderer = renderer;
        self.resources = resources;
        self.last_presented_revision = None;
        Ok(())
    }
}

fn choose_surface_format(formats: &[TextureFormat]) -> Option<TextureFormat> {
    formats
        .iter()
        .copied()
        .find(TextureFormat::is_srgb)
        .or_else(|| formats.first().copied())
}

#[derive(Debug, Error)]
pub(crate) enum GraphicsError {
    #[error("failed to select a WGPU adapter: {0}")]
    Adapter(String),

    #[error("failed to create a WGPU device: {0}")]
    Device(String),

    #[error("failed to create a WGPU surface: {0}")]
    Surface(String),

    #[error("the selected adapter cannot render to the native surface")]
    UnsupportedSurface,

    #[error("cannot create a renderer for a zero-sized surface")]
    EmptySurface,

    #[error("WGPU reported a surface validation failure")]
    SurfaceValidation,

    #[error("Vello Hybrid rendering failed: {0}")]
    Render(String),

    #[error("scene target {frame:?} does not match surface target {surface:?}")]
    FrameSizeMismatch {
        frame: PhysicalSize<u32>,
        surface: PhysicalSize<u32>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_format_prefers_srgb_and_falls_back_to_the_first_format() {
        assert_eq!(
            choose_surface_format(&[TextureFormat::Rgba8Unorm, TextureFormat::Bgra8UnormSrgb]),
            Some(TextureFormat::Bgra8UnormSrgb)
        );
        assert_eq!(
            choose_surface_format(&[TextureFormat::Rgba8Unorm]),
            Some(TextureFormat::Rgba8Unorm)
        );
        assert_eq!(choose_surface_format(&[]), None);
    }
}
