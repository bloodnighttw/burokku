use std::sync::mpsc;

use super::{RenderError, Renderer, SurfaceSize};
use crate::{Canvas, TextSystem};

const BYTES_PER_PIXEL: u32 = 4;

pub(super) struct TestImage {
    pub pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl TestImage {
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = ((y * self.width + x) * BYTES_PER_PIXEL) as usize;
        Some(
            self.pixels[offset..offset + 4]
                .try_into()
                .expect("four bytes"),
        )
    }
}

pub(super) fn draw_to_image(
    renderer: &mut Renderer,
    canvas: &Canvas,
    size: SurfaceSize,
    text_system: &mut TextSystem,
) -> Result<TestImage, RenderError> {
    let texture = renderer
        .gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("render test target"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.surface.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.draw_to_view(&view, canvas, size, text_system)?;

    let row_bytes = size.width * BYTES_PER_PIXEL;
    let padded_row_bytes =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = renderer.gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render test readback"),
        size: u64::from(padded_row_bytes) * u64::from(size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer
        .gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render test readback encoder"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(size.height),
            },
        },
        texture.size(),
    );
    renderer.gpu.queue.submit([encoder.finish()]);

    let (sender, receiver) = mpsc::sync_channel(1);
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = sender.send(result);
    });
    renderer
        .gpu
        .device
        .poll(wgpu::PollType::wait_indefinitely())?;
    receiver
        .recv()
        .map_err(|_| RenderError::ReadbackCallbackDropped)??;

    let mapped = buffer.get_mapped_range(..);
    let mut pixels = Vec::with_capacity((row_bytes * size.height) as usize);
    for row in mapped.chunks_exact(padded_row_bytes as usize) {
        pixels.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(TestImage {
        pixels,
        width: size.width,
        height: size.height,
    })
}
