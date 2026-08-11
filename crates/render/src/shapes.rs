pub mod rect;
pub mod round;
mod rounded_rect;
pub mod stroke;

use crate::{canvas::DrawCommand, shapes::rounded_rect::RoundedRectRenderer};

pub(crate) struct ShapeRenderer {
    rounded_rect: RoundedRectRenderer,
}

impl ShapeRenderer {
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        Self {
            rounded_rect: RoundedRectRenderer::new(device, surface_format),
        }
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: &[DrawCommand],
        canvas_size: [u32; 2],
    ) {
        self.rounded_rect
            .prepare(device, queue, commands, canvas_size);
    }

    pub(crate) fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.rounded_rect.draw(pass);
    }
}
