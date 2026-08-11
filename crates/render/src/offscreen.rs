//! Test-only texture-backed rendering and RGBA8 readback.

use std::sync::mpsc;

use crate::{canvas::DrawList, clip::commands_are_balanced, shapes::ShapeRenderer};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub(crate) struct OffscreenSurface {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    renderer: ShapeRenderer,
    size: [u32; 2],
}

impl OffscreenSurface {
    /// Creates a real GPU-backed offscreen target, or returns `None` when the
    /// test environment has no available WebGPU adapter.
    pub(crate) async fn new(size: [u32; 2]) -> Option<Self> {
        assert!(size[0] > 0 && size[1] > 0);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .ok()?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("render offscreen test device"),
                ..Default::default()
            })
            .await
            .ok()?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render offscreen test texture"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let renderer = ShapeRenderer::new(&device, FORMAT);

        Some(Self {
            _instance: instance,
            device,
            queue,
            texture,
            renderer,
            size,
        })
    }

    pub(crate) async fn render_rgba8(
        &mut self,
        draws: &DrawList,
        clear_color: wgpu::Color,
    ) -> Vec<u8> {
        assert!(commands_are_balanced(draws.commands()));
        self.renderer
            .prepare(&self.device, &self.queue, draws.commands(), self.size);

        let view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render offscreen test encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render offscreen test pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.draw(&mut pass);
        }

        let unpadded_bytes_per_row = self.size[0] * 4;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render offscreen test readback"),
            size: padded_bytes_per_row as u64 * self.size[1] as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender
                .send(result)
                .expect("offscreen map receiver should still exist");
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("offscreen GPU work should complete");
        receiver
            .recv()
            .expect("offscreen map callback should run")
            .expect("offscreen readback buffer should map");

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * self.size[1]) as usize);
        for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
            pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
        }
        drop(mapped);
        readback.unmap();
        pixels
    }

    pub(crate) fn pixel<'pixels>(&self, pixels: &'pixels [u8], x: u32, y: u32) -> &'pixels [u8] {
        assert!(x < self.size[0] && y < self.size[1]);
        let start = ((y * self.size[0] + x) * 4) as usize;
        &pixels[start..start + 4]
    }
}

const fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}
