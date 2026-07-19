use super::RenderError;

pub(super) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<(Self, wgpu::Adapter), RenderError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                force_fallback_adapter: false,
                compatible_surface: surface,
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("render device"),
                ..Default::default()
            })
            .await?;
        Ok((Self { device, queue }, adapter))
    }
}
