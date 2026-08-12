//! Shared WebGPU state used by every canvas.

use crate::{canvas::DrawCommand, variants::fill::FillRenderer};

/// Persistent GPU state shared by canvas drawing code.
///
/// Render pipelines live here so they can be reused across frames. A pipeline
/// is rebuilt if the engine is asked to draw into a different texture format.
pub struct Engine {
    device: wgpu::Device,
    queue: wgpu::Queue,
    fill_renderer: Option<(wgpu::TextureFormat, FillRenderer)>,
}

impl Engine {
    /// Device and queue handles are reference-counted by wgpu, so retaining
    /// clones here is inexpensive.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            fill_renderer: None,
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

    pub(crate) fn prepare_fill(
        &mut self,
        target_format: wgpu::TextureFormat,
        commands: &[DrawCommand],
        canvas_size: [u32; 2],
    ) {
        let renderer_matches_format = self
            .fill_renderer
            .as_ref()
            .is_some_and(|(format, _)| *format == target_format);
        if !renderer_matches_format {
            self.fill_renderer = Some((
                target_format,
                FillRenderer::new(&self.device, target_format),
            ));
        }

        self.fill_renderer
            .as_mut()
            .expect("fill renderer should be initialized")
            .1
            .prepare(&self.device, &self.queue, commands, canvas_size);
    }

    pub(crate) fn fill_renderer(&self) -> &FillRenderer {
        &self
            .fill_renderer
            .as_ref()
            .expect("fill renderer must be prepared before drawing")
            .1
    }
}
