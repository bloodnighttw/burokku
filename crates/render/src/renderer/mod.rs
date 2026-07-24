mod gpu;
mod shape;
mod surface;
mod text;

#[cfg(test)]
mod readback;

use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{Canvas, TextSystem};
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
        Self {
            gpu,
            surface,
            shapes,
            text,
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
            self.text.draw(&mut pass)?;
            self.shapes.draw_overlay(&mut pass);
        }
        let submit_started_at = Instant::now();
        self.gpu.queue.submit([encoder.finish()]);
        let queue_submit = submit_started_at.elapsed();
        self.text.finish_frame();
        Ok(queue_submit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackgroundImage, Border, BoxShadow, BoxStyle, Clip, Color, CornerRadius, Outline,
        RasterImage, Rect, TextStyle, Transform,
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
                    start: Color::from_rgba8(255, 0, 0, 255),
                    end: Color::from_rgba8(0, 0, 255, 255),
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
                shadow: Some(BoxShadow {
                    offset: [5.0, 6.0],
                    blur: 2.0,
                    spread: 1.0,
                    color: Color::from_rgba8(0, 0, 0, 180),
                }),
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
        let gradient_right = image.pixel(45, 18).unwrap();
        assert!(gradient_left[0] > gradient_left[2]);
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
}
