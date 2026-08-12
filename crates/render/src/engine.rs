//! Target-independent frame preparation and GPU command encoding.

use crate::{canvas::DrawList, clip::commands_are_balanced, shapes::ShapeRenderer};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderEngineError {
    UnbalancedClipStack,
}

/// The output attachment used for one encoded frame.
///
/// Window and offscreen targets own their textures and only lend their views to
/// the engine. Keeping target acquisition outside the engine lets the same
/// rendering lifecycle serve swapchain images, multisampled textures, and
/// future compositor-owned scene textures.
pub(crate) struct RenderTarget<'view> {
    pub(crate) color_view: &'view wgpu::TextureView,
    pub(crate) resolve_view: Option<&'view wgpu::TextureView>,
    pub(crate) store: wgpu::StoreOp,
}

/// Persistent target-independent GPU renderer state.
///
/// Target adapters retain responsibility for acquiring and presenting images
/// or reading textures back. The engine owns shared GPU handles, prepares the
/// retained draw list, and encodes the render passes common to every target.
pub(crate) struct RenderEngine {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shape_renderer: ShapeRenderer,
}

impl RenderEngine {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            shape_renderer: ShapeRenderer::new(device, format, sample_count),
        }
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Validates and uploads a retained frame before a target image is acquired.
    pub(crate) fn prepare(
        &mut self,
        draws: &DrawList,
        size: [u32; 2],
    ) -> Result<(), RenderEngineError> {
        if !commands_are_balanced(draws.commands()) {
            return Err(RenderEngineError::UnbalancedClipStack);
        }

        self.shape_renderer
            .prepare(&self.device, &self.queue, draws.commands(), size);
        Ok(())
    }

    /// Encodes the prepared frame without submitting it.
    ///
    /// Submission remains with the target adapter so it can order presentation,
    /// readback, and any target-specific command buffers around the rendered
    /// frame.
    pub(crate) fn encode(
        &self,
        target: RenderTarget<'_>,
        clear_color: wgpu::Color,
    ) -> wgpu::CommandBuffer {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render engine frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render engine frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.color_view,
                    depth_slice: None,
                    resolve_target: target.resolve_view,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: target.store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.shape_renderer.draw(&mut pass);
        }

        encoder.finish()
    }
}
