mod gpu;
mod shape;
mod surface;
mod text;

#[cfg(test)]
mod readback;

use thiserror::Error;

use crate::Canvas;
use gpu::Gpu;
use shape::ShapeRenderer;
use surface::SurfaceState;
use text::TextRenderer;

pub use surface::SurfaceSize;

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
    ) -> Result<(), RenderError> {
        let frame = self.surface.acquire(surface)?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.draw_to_view(&view, canvas, self.surface.size())?;
        frame.present();
        Ok(())
    }

    fn draw_to_view(
        &mut self,
        view: &wgpu::TextureView,
        canvas: &Canvas,
        size: SurfaceSize,
    ) -> Result<(), RenderError> {
        let prepared_shapes = self
            .shapes
            .prepare(&self.gpu.device, &self.gpu.queue, canvas, size);
        self.text
            .prepare(&self.gpu.device, &self.gpu.queue, canvas, size)?;

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
            self.shapes.draw(&prepared_shapes, &mut pass);
            self.text.draw(&mut pass)?;
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.text.finish_frame();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Border, BoxStyle, Color, CornerRadius, Outline, Rect, TextStyle};

    #[test]
    fn renders_box_border_outline_text_and_readback() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = pollster::block_on(Gpu::new(&instance, None)) else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(64, 64),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);
        canvas.draw_box(
            Rect::new(16.0, 16.0, 32.0, 32.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                corner_radius: CornerRadius::all(6.0),
                border: Some(Border::new(3.0, Color::BLACK)),
                outline: Some(Outline::new(2.0, 2.0, Color::from_rgba8(20, 80, 220, 255))),
            },
        );

        let image = readback::draw_to_image(&mut renderer, &canvas, SurfaceSize::new(64, 64))
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
        let text_image =
            readback::draw_to_image(&mut renderer, &text_canvas, SurfaceSize::new(160, 48))
                .expect("text test render");
        assert!(text_image
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] < 220 && pixel[3] > 0));

        drop(adapter);
    }
}
