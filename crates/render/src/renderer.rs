mod composite;
mod gpu;
mod shape;
mod surface;
mod text;

#[cfg(test)]
mod readback;

use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{Canvas, TextSystem};
use composite::{CompositeEffect, CompositeItem, CompositeRenderer};
use gpu::Gpu;
use shape::ShapeRenderer;
use surface::SurfaceState;
use text::TextRenderer;

pub use surface::SurfaceSize;

/// CPU-side timings for rendering and submitting one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderTimings {
    pub total: Duration,
    pub queue_submit: Duration,
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("surface dimensions must both be greater than zero")]
    InvalidSurfaceSize,
    #[error("WebGPU could not find a suitable graphics adapter: {0}")]
    RequestAdapter(#[from] wgpu::RequestAdapterError),
    #[error("WebGPU device creation failed: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("the adapter and surface have no compatible texture format")]
    NoSurfaceFormat,
    #[error("the surface frame timed out")]
    SurfaceTimeout,
    #[error("the surface is currently occluded")]
    SurfaceOccluded,
    #[error("the surface is outdated and must be reconfigured")]
    SurfaceOutdated,
    #[error("the surface was lost and must be recreated")]
    SurfaceLost,
    #[error("surface validation failed")]
    SurfaceValidation,
    #[error("text preparation failed: {0}")]
    PrepareText(#[from] glyphon::PrepareError),
    #[error("text rendering failed: {0}")]
    RenderText(#[from] glyphon::RenderError),
    #[cfg(test)]
    #[error("GPU readback failed: {0}")]
    Readback(#[from] wgpu::BufferAsyncError),
    #[cfg(test)]
    #[error("GPU polling failed: {0}")]
    Poll(#[from] wgpu::PollError),
    #[cfg(test)]
    #[error("GPU readback callback was dropped")]
    ReadbackCallbackDropped,
}

/// Reusable WebGPU drawing state for a surface owned by the application.
pub struct Renderer {
    gpu: Gpu,
    surface: SurfaceState,
    shapes: ShapeRenderer,
    text: TextRenderer,
    composites: CompositeRenderer,
}

impl Renderer {
    /// Creates drawing resources compatible with `surface` and configures it.
    ///
    /// The application remains responsible for keeping both its window and the
    /// surface alive and passes the surface back to [`Self::render`].
    pub async fn new(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        size: SurfaceSize,
    ) -> Result<Self, RenderError> {
        if size.is_zero() {
            return Err(RenderError::InvalidSurfaceSize);
        }
        let (gpu, adapter) = Gpu::new(instance, Some(surface)).await?;
        let surface_state = SurfaceState::new(surface, &adapter, &gpu.device, size)?;
        Ok(Self::from_gpu(gpu, surface_state))
    }

    fn from_gpu(gpu: Gpu, surface: SurfaceState) -> Self {
        let shapes = ShapeRenderer::new(&gpu.device, surface.format());
        let text = TextRenderer::new(&gpu.device, &gpu.queue, surface.format());
        let composites = CompositeRenderer::new(&gpu.device, surface.format());
        Self {
            gpu,
            surface,
            shapes,
            text,
            composites,
        }
    }

    pub fn size(&self) -> SurfaceSize {
        self.surface.size()
    }

    /// Reconfigures the application-owned surface after a window resize.
    /// Zero-sized windows (for example while minimized) are ignored.
    pub fn resize(&mut self, surface: &wgpu::Surface<'_>, size: SurfaceSize) {
        if !size.is_zero() {
            self.surface.resize(surface, &self.gpu.device, size);
        }
    }

    /// Draws and presents one frame on the application-owned surface.
    pub fn render(
        &mut self,
        surface: &wgpu::Surface<'_>,
        canvas: &Canvas,
        text_system: &mut TextSystem,
    ) -> Result<(), RenderError> {
        self.render_with_pre_present(surface, canvas, text_system, || {})
    }

    /// Draws one frame and notifies the window system immediately before it is
    /// presented. This keeps redraw scheduling synchronized with compositors
    /// that use presentation notifications.
    pub fn render_with_pre_present(
        &mut self,
        surface: &wgpu::Surface<'_>,
        canvas: &Canvas,
        text_system: &mut TextSystem,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), RenderError> {
        self.render_timed_with_pre_present(surface, canvas, text_system, on_pre_present)
            .map(|_| ())
    }

    /// Draws and presents one frame while returning the CPU time spent sending
    /// its command buffer to the GPU queue.
    pub fn render_timed_with_pre_present(
        &mut self,
        surface: &wgpu::Surface<'_>,
        canvas: &Canvas,
        text_system: &mut TextSystem,
        on_pre_present: impl FnOnce(),
    ) -> Result<RenderTimings, RenderError> {
        let render_started_at = Instant::now();
        let frame = self.surface.acquire(surface)?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let queue_submit = self.draw_to_view(&view, canvas, self.surface.size(), text_system)?;
        on_pre_present();
        frame.present();
        Ok(RenderTimings {
            total: render_started_at.elapsed(),
            queue_submit,
        })
    }

    fn draw_to_view(
        &mut self,
        view: &wgpu::TextureView,
        canvas: &Canvas,
        size: SurfaceSize,
        text_system: &mut TextSystem,
    ) -> Result<Duration, RenderError> {
        let bytes_per_target = u64::from(size.width)
            .saturating_mul(u64::from(size.height))
            .saturating_mul(u64::from(
                self.surface.format().block_copy_size(None).unwrap_or(16),
            ));
        let mut budget = GroupBudget {
            used: 0,
            maximum: (128 * 1024 * 1024_u64)
                .min(self.gpu.device.limits().max_buffer_size.saturating_mul(2)),
            bytes_per_target,
            maximum_depth: 32,
        };
        self.draw_canvas_to_view(view, canvas, size, text_system, &mut budget, 0)
    }

    fn draw_canvas_to_view(
        &mut self,
        view: &wgpu::TextureView,
        canvas: &Canvas,
        size: SurfaceSize,
        text_system: &mut TextSystem,
        budget: &mut GroupBudget,
        depth: usize,
    ) -> Result<Duration, RenderError> {
        let mut composite_items = Vec::<CompositeItem>::new();
        let mut reserved = 0_u64;
        if depth < budget.maximum_depth {
            for command in canvas.commands() {
                let crate::DrawCommand::Group {
                    canvas,
                    origin,
                    transform,
                    opacity,
                    clips,
                } = command
                else {
                    continue;
                };
                if !budget.reserve() {
                    continue;
                }
                let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("render transient group target"),
                    size: wgpu::Extent3d {
                        width: size.width,
                        height: size.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.surface.format(),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let group_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.draw_canvas_to_view(
                    &group_view,
                    canvas,
                    size,
                    text_system,
                    budget,
                    depth + 1,
                )?;
                let item = self.composites.item(
                    &self.gpu.device,
                    texture,
                    size,
                    CompositeEffect {
                        origin: *origin,
                        transform: *transform,
                        opacity: *opacity,
                        clips: clips.clone(),
                    },
                );
                if let Some(item) = item {
                    composite_items.push(item);
                    reserved = reserved.saturating_add(budget.bytes_per_target);
                } else {
                    budget.release(budget.bytes_per_target);
                }
            }
        }
        self.shapes
            .prepare(&self.gpu.device, &self.gpu.queue, canvas, size);
        self.text
            .prepare(&self.gpu.device, &self.gpu.queue, canvas, size, text_system)?;

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render drawing pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(canvas.clear_color.as_wgpu_clear()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.shapes.draw_base(&mut pass);
            self.composites.draw(&mut pass, &composite_items, size);
            self.text.draw(&mut pass)?;
            self.shapes.draw_overlay(&mut pass);
        }
        let submit_started_at = Instant::now();
        self.gpu.queue.submit([encoder.finish()]);
        let queue_submit = submit_started_at.elapsed();
        self.text.finish_frame();
        budget.release(reserved);
        Ok(queue_submit)
    }
}

struct GroupBudget {
    used: u64,
    maximum: u64,
    bytes_per_target: u64,
    maximum_depth: usize,
}

impl GroupBudget {
    fn reserve(&mut self) -> bool {
        let Some(next) = self.used.checked_add(self.bytes_per_target) else {
            return false;
        };
        if self.bytes_per_target == 0 || next > self.maximum {
            return false;
        }
        self.used = next;
        true
    }

    fn release(&mut self, bytes: u64) {
        self.used = self.used.saturating_sub(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackgroundImage, Border, BoxShadow, BoxStyle, Clip, Color, CornerRadius, GradientStop,
        Outline, RasterImage, Rect, TextStyle, Transform,
    };

    #[tokio::test(flavor = "current_thread")]
    async fn renders_box_border_outline_text_and_readback() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_box(
            Rect::new(16.0, 16.0, 32.0, 32.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                corner_radius: CornerRadius::all(6.0),
                border: Some(Border::new(3.0, Color::BLACK)),
                outline: Some(Outline::new(2.0, 2.0, Color::from_rgba8(20, 80, 220, 255))),
                ..BoxStyle::default()
            },
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen test render");
        assert_eq!(image.pixels.len(), 64 * 64 * 4);
        assert_eq!(image.pixel(0, 0), Some([255, 255, 255, 255]));
        let center = image.pixel(32, 32).expect("center pixel");
        assert!(center[0] > 180 && center[1] < 80 && center[2] < 90);
        assert!(
            image
                .pixels
                .chunks_exact(4)
                .filter(|pixel| pixel[0] < 60 && pixel[1] < 60 && pixel[2] < 60 && pixel[3] > 200)
                .count()
                > 20
        );
        assert!(
            image
                .pixels
                .chunks_exact(4)
                .filter(|pixel| {
                    pixel[2] > pixel[0].saturating_add(60)
                        && pixel[2] > pixel[1].saturating_add(40)
                        && pixel[3] > 100
                })
                .count()
                > 20
        );

        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(160, 48),
        );
        renderer.surface = surface;
        let mut text_canvas = Canvas::new().with_clear_color(Color::WHITE);
        text_canvas.draw_text(
            Rect::new(4.0, 4.0, 152.0, 40.0),
            "Burokku",
            TextStyle {
                font_size: 24.0,
                line_height: 30.0,
                ..TextStyle::default()
            },
        );
        let text_image = readback::draw_to_image(
            &mut renderer,
            &text_canvas,
            SurfaceSize::new(160, 48),
            &mut text_system,
        )
        .expect("text test render");
        assert!(text_image
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] < 220 && pixel[3] > 0));
        let cached_text_image = readback::draw_to_image(
            &mut renderer,
            &text_canvas,
            SurfaceSize::new(160, 48),
            &mut text_system,
        )
        .expect("cached text test render");
        assert_eq!(cached_text_image.pixels, text_image.pixels);

        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clips_shape_pixels_to_a_rounded_command_clip() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_box_clipped(
            Rect::new(4.0, 4.0, 56.0, 56.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                ..BoxStyle::default()
            },
            Clip::new(Rect::new(24.0, 24.0, 16.0, 16.0), CornerRadius::all(8.0)),
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen clipped render");

        assert_eq!(image.pixel(16, 32), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(32, 16), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(24, 24), Some([255, 255, 255, 255]));
        let inside = image.pixel(32, 32).expect("inside clipped shape");
        assert!(inside[0] > 180 && inside[1] < 80 && inside[2] < 90);
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clips_shape_pixels_with_an_affine_transformed_clip() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        let angle = 45.0_f32.to_radians();
        let (sin, cos) = angle.sin_cos();
        let mut clip = Clip::rectangular(Rect::new(24.0, 16.0, 16.0, 32.0));
        clip.transform = [cos, sin, -sin, cos, 0.0, 0.0];
        canvas.draw_box_clipped(
            Rect::new(4.0, 4.0, 56.0, 56.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                ..BoxStyle::default()
            },
            clip,
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen transformed clip render");
        let center = image.pixel(32, 32).unwrap();
        assert!(center[0] > 180 && center[1] < 80);
        assert_eq!(image.pixel(20, 20), Some([255, 255, 255, 255]));
        assert_eq!(image.pixel(12, 32), Some([255, 255, 255, 255]));
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn renders_gradient_opacity_transform_and_shadow_pixels() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(96, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_box(
            Rect::new(8.0, 8.0, 40.0, 20.0),
            BoxStyle {
                background_image: Some(BackgroundImage::LinearGradient {
                    direction: [1.0, 0.0],
                    stops: vec![
                        GradientStop {
                            color: Color::from_rgba8(255, 0, 0, 255),
                            position: 0.0,
                        },
                        GradientStop {
                            color: Color::from_rgba8(0, 255, 0, 255),
                            position: 0.5,
                        },
                        GradientStop {
                            color: Color::from_rgba8(0, 0, 255, 255),
                            position: 1.0,
                        },
                    ],
                }),
                opacity: 0.5,
                ..BoxStyle::default()
            },
        );
        canvas.draw_box(
            Rect::new(8.0, 38.0, 12.0, 12.0),
            BoxStyle {
                background: Color::from_rgba8(0, 180, 0, 255),
                transform: Transform {
                    matrix: [1.0, 0.0, 0.0, 1.0, 20.0, 0.0],
                },
                ..BoxStyle::default()
            },
        );
        canvas.draw_box(
            Rect::new(64.0, 10.0, 12.0, 12.0),
            BoxStyle {
                background: Color::from_rgba8(240, 180, 0, 255),
                shadows: vec![BoxShadow {
                    offset: [5.0, 6.0],
                    blur: 2.0,
                    spread: 1.0,
                    color: Color::from_rgba8(0, 0, 0, 180),
                    inset: false,
                }],
                ..BoxStyle::default()
            },
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(96, 64),
            &mut text_system,
        )
        .expect("off-screen paint effects render");

        let gradient_left = image.pixel(10, 18).unwrap();
        let gradient_middle = image.pixel(28, 18).unwrap();
        let gradient_right = image.pixel(45, 18).unwrap();
        assert!(gradient_left[0] > gradient_left[2]);
        assert!(gradient_middle[1] > gradient_middle[0] && gradient_middle[1] > gradient_middle[2]);
        assert!(gradient_right[2] > gradient_right[0]);
        assert!(gradient_left[1] > 120, "opacity must reveal white below");
        assert_eq!(image.pixel(12, 44), Some([255, 255, 255, 255]));
        let transformed = image.pixel(34, 44).unwrap();
        assert!(transformed[1] > transformed[0].saturating_add(80));
        let shadow = image.pixel(78, 25).unwrap();
        assert!(shadow[0] < 245 && shadow[1] < 245 && shadow[2] < 245);
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn uploads_and_samples_raster_background_images() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 56),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let image = RasterImage::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_box(
            Rect::new(8.0, 8.0, 48.0, 16.0),
            BoxStyle {
                background_image: Some(BackgroundImage::Raster(image)),
                ..BoxStyle::default()
            },
        );
        let second = RasterImage::new(1, 2, vec![0, 255, 0, 255, 255, 255, 0, 255]).unwrap();
        canvas.draw_box(
            Rect::new(8.0, 32.0, 48.0, 16.0),
            BoxStyle {
                background_image: Some(BackgroundImage::Raster(second)),
                ..BoxStyle::default()
            },
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 56),
            &mut text_system,
        )
        .expect("off-screen raster background render");
        let left = image.pixel(12, 16).unwrap();
        let right = image.pixel(52, 16).unwrap();
        assert!(left[0] > 220 && left[2] < 40, "{left:?}");
        assert!(right[2] > 220 && right[0] < 40, "{right:?}");
        let second_top = image.pixel(32, 34).unwrap();
        let second_bottom = image.pixel(32, 46).unwrap();
        assert!(second_top[1] > 220 && second_top[0] < 40, "{second_top:?}");
        assert!(
            second_bottom[0] > 220 && second_bottom[1] > 220,
            "{second_bottom:?}"
        );
        assert_eq!(image.pixel(2, 2), Some([255, 255, 255, 255]));
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn renders_multiple_outer_and_inset_box_shadows() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_box(
            Rect::new(16.0, 16.0, 32.0, 32.0),
            BoxStyle {
                background: Color::from_rgba8(250, 220, 40, 255),
                shadows: vec![
                    BoxShadow {
                        offset: [5.0, 5.0],
                        blur: 2.0,
                        spread: 1.0,
                        color: Color::from_rgba8(0, 0, 0, 180),
                        inset: false,
                    },
                    BoxShadow {
                        offset: [0.0, 0.0],
                        blur: 3.0,
                        spread: 2.0,
                        color: Color::from_rgba8(180, 0, 0, 220),
                        inset: true,
                    },
                ],
                ..BoxStyle::default()
            },
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen multiple shadow render");
        let center = image.pixel(32, 32).unwrap();
        let inset_edge = image.pixel(18, 32).unwrap();
        let outer = image.pixel(51, 51).unwrap();
        assert!(center[0] > 200 && center[1] > 170);
        assert!(inset_edge[0] > inset_edge[1].saturating_add(40));
        assert!(outer[0] < 245 && outer[1] < 245 && outer[2] < 245);
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn composites_overlapping_descendants_before_applying_group_opacity() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 40),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut group = Canvas::new();
        group.draw_box(
            Rect::new(8.0, 8.0, 28.0, 24.0),
            BoxStyle {
                background: Color::from_rgba8(255, 0, 0, 255),
                ..BoxStyle::default()
            },
        );
        group.draw_box(
            Rect::new(28.0, 8.0, 28.0, 24.0),
            BoxStyle {
                background: Color::from_rgba8(0, 0, 255, 255),
                ..BoxStyle::default()
            },
        );
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_group(group, [32.0, 20.0], Transform::IDENTITY, 0.5, []);

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 40),
            &mut text_system,
        )
        .expect("off-screen opacity group render");
        let red_only = image.pixel(16, 20).unwrap();
        let overlap = image.pixel(32, 20).unwrap();
        assert!((i16::from(red_only[1]) - i16::from(overlap[1])).abs() < 8);
        assert!(overlap[2] > overlap[0].saturating_add(80));
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn group_composite_preserves_intrinsic_descendant_alpha() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(40, 40),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let style = BoxStyle {
            background: Color::from_rgba8(255, 0, 0, 128),
            ..BoxStyle::default()
        };
        let mut direct = Canvas::new().with_clear_color(Color::WHITE);
        direct.draw_box(Rect::new(8.0, 8.0, 24.0, 24.0), style.clone());
        let direct_image = readback::draw_to_image(
            &mut renderer,
            &direct,
            SurfaceSize::new(40, 40),
            &mut text_system,
        )
        .expect("direct alpha render");

        let mut group = Canvas::new();
        group.draw_box(Rect::new(8.0, 8.0, 24.0, 24.0), style);
        let mut grouped = Canvas::new().with_clear_color(Color::WHITE);
        grouped.draw_group(group, [20.0, 20.0], Transform::IDENTITY, 1.0, []);
        let grouped_image = readback::draw_to_image(
            &mut renderer,
            &grouped,
            SurfaceSize::new(40, 40),
            &mut text_system,
        )
        .expect("grouped alpha render");

        let direct_pixel = direct_image.pixel(20, 20).unwrap();
        let grouped_pixel = grouped_image.pixel(20, 20).unwrap();
        for (direct, grouped) in direct_pixel.into_iter().zip(grouped_pixel) {
            assert!((i16::from(direct) - i16::from(grouped)).abs() <= 2);
        }
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn affine_group_rotation_applies_to_rasterized_glyph_pixels() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut group = Canvas::new();
        group.draw_text(
            Rect::new(20.0, 8.0, 24.0, 48.0),
            "I",
            TextStyle {
                font_size: 40.0,
                line_height: 48.0,
                ..TextStyle::default()
            },
        );
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_group(
            group,
            [32.0, 32.0],
            Transform {
                matrix: [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
            },
            1.0,
            [],
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen affine text group render");
        let mut min_x = 64;
        let mut max_x = 0;
        let mut min_y = 64;
        let mut max_y = 0;
        for y in 0..64 {
            for x in 0..64 {
                let pixel = image.pixel(x, y).unwrap();
                if pixel[0] < 180 {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(max_x > min_x && max_y > min_y);
        assert!(max_x - min_x > max_y - min_y);
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn affine_group_transform_moves_its_clipped_pixels_together() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut group = Canvas::new();
        group.draw_box_clipped(
            Rect::new(8.0, 8.0, 48.0, 48.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                ..BoxStyle::default()
            },
            Clip::rectangular(Rect::new(28.0, 12.0, 8.0, 40.0)),
        );
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_group(
            group,
            [32.0, 32.0],
            Transform {
                matrix: [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
            },
            1.0,
            [],
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen transformed clipped group");
        let horizontal = image.pixel(16, 32).unwrap();
        assert!(horizontal[0] > 180 && horizontal[1] < 80);
        assert_eq!(image.pixel(32, 16), Some([255, 255, 255, 255]));
        drop(adapter);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn group_composite_applies_affine_clip_shape_not_only_its_bounds() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut group = Canvas::new();
        group.draw_box(
            Rect::new(4.0, 4.0, 56.0, 56.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                ..BoxStyle::default()
            },
        );
        let mut clip = Clip::rectangular(Rect::new(12.0, 26.0, 40.0, 12.0));
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        clip.transform = [diagonal, diagonal, -diagonal, diagonal, 0.0, 0.0];
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_group(group, [32.0, 32.0], Transform::IDENTITY, 1.0, [clip]);

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(64, 64),
            &mut text_system,
        )
        .expect("off-screen affine group clip");
        let center = image.pixel(32, 32).unwrap();
        assert!(center[0] > 180 && center[1] < 80);
        assert_eq!(image.pixel(18, 46), Some([255, 255, 255, 255]));
        drop(adapter);
    }

    #[test]
    fn group_budget_rejects_excess_targets_and_depth_is_explicit() {
        let mut budget = GroupBudget {
            used: 0,
            maximum: 128,
            bytes_per_target: 64,
            maximum_depth: 3,
        };
        assert!(budget.reserve());
        assert!(budget.reserve());
        assert!(!budget.reserve());
        budget.release(64);
        assert!(budget.reserve());
        assert_eq!(budget.maximum_depth, 3);
    }
}
