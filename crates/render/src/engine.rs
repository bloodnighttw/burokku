//! Shared WebGPU state used by every canvas.

/// Persistent GPU state shared by canvas drawing code.
///
/// Texture format and multisampling become relevant here when render pipelines
/// are added. Until then, the engine only owns the device and queue.
pub struct Engine {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Engine {
    /// Device and queue handles are reference-counted by wgpu, so retaining
    /// clones here is inexpensive.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub(crate) fn create_command_encoder(&self, label: Option<&str>) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label })
    }

    pub(crate) fn submit(&self, commands: wgpu::CommandBuffer) -> wgpu::SubmissionIndex {
        self.queue.submit([commands])
    }
}
