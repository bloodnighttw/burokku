use super::RenderError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceSize {
    pub width: u32,
    pub height: u32,
}

impl SurfaceSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_zero(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

pub(super) struct SurfaceState {
    config: wgpu::SurfaceConfiguration,
}

impl SurfaceState {
    pub fn new(
        surface: &wgpu::Surface<'_>,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        size: SurfaceSize,
    ) -> Result<Self, RenderError> {
        let capabilities = surface.get_capabilities(adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RenderError::NoSurfaceFormat)?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .unwrap_or(capabilities.present_modes[0]);
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(device, &config);
        Ok(Self { config })
    }

    #[cfg(test)]
    pub fn offscreen(format: wgpu::TextureFormat, size: SurfaceSize) -> Self {
        Self {
            config: wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> SurfaceSize {
        SurfaceSize::new(self.config.width, self.config.height)
    }

    pub fn resize(
        &mut self,
        surface: &wgpu::Surface<'_>,
        device: &wgpu::Device,
        size: SurfaceSize,
    ) {
        self.config.width = size.width;
        self.config.height = size.height;
        surface.configure(device, &self.config);
    }

    pub fn acquire(
        &self,
        surface: &wgpu::Surface<'_>,
    ) -> Result<wgpu::SurfaceTexture, RenderError> {
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
            wgpu::CurrentSurfaceTexture::Timeout => Err(RenderError::SurfaceTimeout),
            wgpu::CurrentSurfaceTexture::Occluded => Err(RenderError::SurfaceOccluded),
            wgpu::CurrentSurfaceTexture::Outdated => Err(RenderError::SurfaceOutdated),
            wgpu::CurrentSurfaceTexture::Lost => Err(RenderError::SurfaceLost),
            wgpu::CurrentSurfaceTexture::Validation => Err(RenderError::SurfaceValidation),
        }
    }
}
