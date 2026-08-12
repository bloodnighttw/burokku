//! Native texture-backed rendering and RGBA8 readback.

use std::sync::mpsc;

use crate::{
    canvas::DrawList,
    engine::{RenderEngine, RenderTarget},
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SAMPLE_COUNT: u32 = 4;

/// A reusable GPU-backed canvas that renders into an in-memory RGBA8 texture.
pub struct OffscreenSurface {
    _instance: wgpu::Instance,
    multisample_texture: wgpu::Texture,
    resolve_texture: wgpu::Texture,
    engine: RenderEngine,
    size: [u32; 2],
}

impl OffscreenSurface {
    /// Creates a GPU-backed offscreen target.
    ///
    /// Returns `None` when the environment has no compatible WebGPU adapter.
    pub async fn new(size: [u32; 2]) -> Option<Self> {
        assert!(size[0] > 0 && size[1] > 0);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .ok()?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("render offscreen device"),
                ..Default::default()
            })
            .await
            .ok()?;
        let resolve_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render offscreen resolve texture"),
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
        let multisample_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render offscreen 4x MSAA texture"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let engine = RenderEngine::new(&device, &queue, FORMAT, SAMPLE_COUNT);

        Some(Self {
            _instance: instance,
            multisample_texture,
            resolve_texture,
            engine,
            size,
        })
    }

    /// Renders `draws` and returns tightly packed, row-major RGBA8 pixels.
    pub async fn render_rgba8(&mut self, draws: &DrawList, clear_color: wgpu::Color) -> Vec<u8> {
        self.engine
            .prepare(draws, self.size)
            .expect("draw commands must contain balanced clip scopes");

        let multisample_view = self
            .multisample_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let resolve_view = self
            .resolve_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let render_commands = self.engine.encode(
            RenderTarget {
                color_view: &multisample_view,
                resolve_view: Some(&resolve_view),
                store: wgpu::StoreOp::Discard,
            },
            clear_color,
        );

        let mut readback_encoder =
            self.engine
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("render offscreen readback encoder"),
                });

        let unpadded_bytes_per_row = self.size[0] * 4;
        let padded_bytes_per_row =
            align_to(unpadded_bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.engine.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("render offscreen readback"),
            size: padded_bytes_per_row as u64 * self.size[1] as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        readback_encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.resolve_texture,
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

        self.engine
            .queue()
            .submit([render_commands, readback_encoder.finish()]);
        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender
                .send(result)
                .expect("offscreen map receiver should still exist");
        });
        self.engine
            .device()
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

    #[cfg(test)]
    pub(crate) fn pixel<'pixels>(&self, pixels: &'pixels [u8], x: u32, y: u32) -> &'pixels [u8] {
        assert!(x < self.size[0] && y < self.size[1]);
        let start = ((y * self.size[0] + x) * 4) as usize;
        &pixels[start..start + 4]
    }
}

const fn align_to(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        shapes::rect::{DrawRectExt, Rect},
        wgsl::{WgslBackdrop, WgslBlurredBackdrop, WgslRaster},
    };

    const SOLID_RASTER: &str = r#"
fn raster_main(_input: WgslInput, params: array<vec4<f32>, 1>) -> vec4<f32> {
    return params[0];
}
"#;

    const INVERT_TINT_BACKDROP: &str = r#"
fn backdrop_main(input: WgslInput, params: array<vec4<f32>, 1>) -> vec4<f32> {
    let prior = sample_backdrop(input.screen_uv);
    return vec4<f32>((vec3<f32>(1.0) - prior.rgb) * params[0].rgb, prior.a);
}
"#;

    const BLURRED_BACKDROP: &str = r#"
fn backdrop_main(input: WgslInput, _params: array<vec4<f32>, 1>) -> vec4<f32> {
    return vec4<f32>(sample_backdrop(input.screen_uv).rgb, 1.0);
}
"#;

    #[tokio::test]
    async fn empty_frame_still_clears_and_resolves_the_output() {
        let Some(mut surface) = OffscreenSurface::new([4, 4]).await else {
            eprintln!("skipping offscreen clear test: no WebGPU adapter available");
            return;
        };

        let pixels = surface
            .render_rgba8(&DrawList::new(), wgpu::Color::GREEN)
            .await;

        assert!(pixels
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 255, 0, 255]));
    }

    #[tokio::test]
    async fn successive_frames_do_not_reuse_previous_batches_or_pixels() {
        let Some(mut surface) = OffscreenSurface::new([8, 8]).await else {
            eprintln!("skipping offscreen frame reuse test: no WebGPU adapter available");
            return;
        };
        let mut first_frame = DrawList::new();
        first_frame.draw_rect(Rect::new(0.0, 0.0, 8.0, 8.0), wgpu::Color::RED);

        let first_pixels = surface.render_rgba8(&first_frame, wgpu::Color::BLACK).await;
        assert_eq!(surface.pixel(&first_pixels, 4, 4), [255, 0, 0, 255]);

        let second_pixels = surface
            .render_rgba8(&DrawList::new(), wgpu::Color::BLUE)
            .await;
        assert!(second_pixels
            .chunks_exact(4)
            .all(|pixel| pixel == [0, 0, 255, 255]));
    }

    #[tokio::test]
    async fn wgsl_raster_preserves_interleaved_draw_order() {
        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping WGSL raster test: no WebGPU adapter available");
            return;
        };
        let shader = WgslRaster::<1>::new("solid raster test", SOLID_RASTER).unwrap();
        let mut draws = DrawList::new();
        draws.draw_rect(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED);
        shader.draw(
            &mut draws,
            Rect::new(2.0, 2.0, 12.0, 12.0),
            [[0.0, 0.0, 1.0, 1.0]],
        );
        draws.draw_rect(Rect::new(4.0, 4.0, 8.0, 8.0), wgpu::Color::GREEN);
        shader.draw(
            &mut draws,
            Rect::new(6.0, 6.0, 4.0, 4.0),
            [[1.0, 1.0, 1.0, 1.0]],
        );

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLACK).await;

        assert_eq!(surface.pixel(&pixels, 1, 1), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 3, 3), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 5, 5), [0, 255, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 7, 7), [255, 255, 255, 255]);
    }

    #[tokio::test]
    async fn backdrop_reads_the_scene_and_later_raster_stays_on_top() {
        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping WGSL backdrop test: no WebGPU adapter available");
            return;
        };
        let effect =
            WgslBackdrop::<1>::new("invert tint backdrop test", INVERT_TINT_BACKDROP).unwrap();
        let mut draws = DrawList::new();
        draws.draw_rect(Rect::new(0.0, 0.0, 8.0, 16.0), wgpu::Color::RED);
        effect.draw(
            &mut draws,
            Rect::new(0.0, 0.0, 16.0, 16.0),
            [[1.0, 0.0, 1.0, 1.0]],
        );
        draws.draw_rect(Rect::new(4.0, 4.0, 8.0, 8.0), wgpu::Color::GREEN);

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 2, 2), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 14, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 6, 6), [0, 255, 0, 255]);
    }

    #[tokio::test]
    async fn blurred_backdrop_uses_private_intermediates_without_leaking() {
        let Some(mut surface) = OffscreenSurface::new([16, 8]).await else {
            eprintln!("skipping blurred WGSL backdrop test: no WebGPU adapter available");
            return;
        };
        let effect =
            WgslBlurredBackdrop::<1>::new("Gaussian backdrop test", BLURRED_BACKDROP).unwrap();
        let mut draws = DrawList::new();
        draws.draw_rect(Rect::new(0.0, 0.0, 8.0, 8.0), wgpu::Color::RED);
        draws.draw_rect(Rect::new(8.0, 0.0, 8.0, 8.0), wgpu::Color::BLUE);
        effect.draw(&mut draws, Rect::new(4.0, 0.0, 8.0, 8.0), 2.0, [[0.0; 4]]);

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLACK).await;

        assert_eq!(surface.pixel(&pixels, 2, 4), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 14, 4), [0, 0, 255, 255]);
        let mixed = surface.pixel(&pixels, 7, 4);
        assert!(mixed[0] > 0 && mixed[2] > 0);
    }

    #[tokio::test]
    async fn clipped_and_consecutive_backdrops_sample_the_latest_scene() {
        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping WGSL backdrop chain test: no WebGPU adapter available");
            return;
        };
        let effect =
            WgslBackdrop::<1>::new("invert tint backdrop chain", INVERT_TINT_BACKDROP).unwrap();
        let mut clipped = DrawList::new();
        clipped.draw_rect(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED);
        clipped.with_clip(Rect::new(4.0, 4.0, 8.0, 8.0), |draws| {
            effect.draw(
                draws,
                Rect::new(0.0, 0.0, 16.0, 16.0),
                [[1.0, 1.0, 1.0, 1.0]],
            );
        });
        let clipped_pixels = surface.render_rgba8(&clipped, wgpu::Color::BLACK).await;
        assert_eq!(surface.pixel(&clipped_pixels, 2, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&clipped_pixels, 8, 8), [0, 255, 255, 255]);

        let mut chained = DrawList::new();
        chained.draw_rect(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED);
        effect.draw(
            &mut chained,
            Rect::new(0.0, 0.0, 16.0, 16.0),
            [[1.0, 1.0, 1.0, 1.0]],
        );
        effect.draw(
            &mut chained,
            Rect::new(0.0, 0.0, 16.0, 16.0),
            [[1.0, 0.0, 1.0, 1.0]],
        );
        let chained_pixels = surface.render_rgba8(&chained, wgpu::Color::BLACK).await;
        assert_eq!(surface.pixel(&chained_pixels, 8, 8), [255, 0, 0, 255]);

        let mut clear_only = DrawList::new();
        effect.draw(
            &mut clear_only,
            Rect::new(0.0, 0.0, 16.0, 16.0),
            [[1.0, 1.0, 1.0, 1.0]],
        );
        let clear_pixels = surface.render_rgba8(&clear_only, wgpu::Color::BLUE).await;
        assert_eq!(surface.pixel(&clear_pixels, 8, 8), [255, 255, 0, 255]);
    }
}
