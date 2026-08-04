mod composite;
mod gpu;
mod shape;
mod surface;
mod text;

#[cfg(test)]
mod readback;

use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{
    BoxDecoration, BoxStyle, Canvas, Color, DecorationStyle, DrawCommand, PaintLayer, Rect,
    TextStyle, TextSystem, Transform,
};
use composite::{CompositeEffect, CompositeRenderer};
use gpu::Gpu;
use shape::ShapeRenderer;
use surface::SurfaceState;
use text::TextRenderer;

pub use surface::SurfaceSize;

const MAX_GROUP_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug)]
struct TargetViewport {
    size: SurfaceSize,
    origin: [f32; 2],
}

impl TargetViewport {
    const fn surface(size: SurfaceSize) -> Self {
        Self {
            size,
            origin: [0.0, 0.0],
        }
    }

    fn rect(self) -> Rect {
        Rect::new(
            self.origin[0],
            self.origin[1],
            self.size.width as f32,
            self.size.height as f32,
        )
    }
}

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
    #[error("a stacking group has more clips than the GPU can accept")]
    TooManyGroupClips,
    #[error("stacking groups are nested more than {MAX_GROUP_DEPTH} levels deep")]
    GroupNestingTooDeep,
    #[error("stacking group target dimensions exceed the GPU texture limit")]
    GroupTargetTooLarge,
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
        self.draw_canvas_to_view(
            view,
            canvas,
            TargetViewport::surface(size),
            text_system,
            0,
            false,
        )
    }

    fn draw_canvas_to_view(
        &mut self,
        view: &wgpu::TextureView,
        canvas: &Canvas,
        viewport: TargetViewport,
        text_system: &mut TextSystem,
        depth: usize,
        already_initialized: bool,
    ) -> Result<Duration, RenderError> {
        if depth > MAX_GROUP_DEPTH {
            return Err(RenderError::GroupNestingTooDeep);
        }

        let mut queue_submit = Duration::ZERO;
        let mut target_initialized = already_initialized;
        let mut shapes_prepared = false;

        // Each child group is rendered and composited immediately. Sibling
        // targets therefore do not accumulate in GPU memory while preserving
        // the existing shape -> group -> text order inside every paint layer.
        for layer in PaintLayer::ALL {
            if canvas_has_shapes_in_layer(canvas, layer) {
                if !shapes_prepared {
                    self.shapes
                        .prepare(&self.gpu.device, &self.gpu.queue, canvas, viewport);
                    shapes_prepared = true;
                }
                let mut encoder = self.create_encoder("render shape layer encoder");
                {
                    let mut pass = begin_drawing_pass(
                        &mut encoder,
                        view,
                        canvas,
                        target_initialized,
                        "render shape layer",
                    );
                    self.shapes.draw_layer(&mut pass, layer);
                }
                queue_submit += self.submit(encoder);
                target_initialized = true;
            }

            for command in canvas.commands() {
                let DrawCommand::Group {
                    layer: command_layer,
                    canvas: group,
                    origin,
                    transform,
                    opacity,
                    clips,
                } = command
                else {
                    continue;
                };
                if *command_layer != layer {
                    continue;
                }
                if group_can_draw_directly(group, *transform, *opacity, clips) {
                    if !target_initialized {
                        let mut encoder = self.create_encoder("render parent clear encoder");
                        drop(begin_drawing_pass(
                            &mut encoder,
                            view,
                            canvas,
                            false,
                            "render parent clear",
                        ));
                        queue_submit += self.submit(encoder);
                        target_initialized = true;
                    }
                    queue_submit += self.draw_canvas_to_view(
                        view,
                        group,
                        viewport,
                        text_system,
                        depth + 1,
                        true,
                    )?;
                    shapes_prepared = false;
                    continue;
                }
                let Some(source) = group_target_viewport(
                    group,
                    viewport,
                    self.gpu.device.limits().max_texture_dimension_2d,
                )?
                else {
                    continue;
                };
                let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("render bounded transient group target"),
                    size: wgpu::Extent3d {
                        width: source.size.width,
                        height: source.size.height,
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
                queue_submit += self.draw_canvas_to_view(
                    &group_view,
                    group,
                    source,
                    text_system,
                    depth + 1,
                    false,
                )?;
                shapes_prepared = false;

                let item = self.composites.item(
                    &self.gpu.device,
                    texture,
                    viewport,
                    source,
                    CompositeEffect {
                        origin: *origin,
                        transform: *transform,
                        opacity: *opacity,
                        clips: clips.clone(),
                    },
                )?;
                let mut encoder = self.create_encoder("render group composite encoder");
                {
                    let mut pass = begin_drawing_pass(
                        &mut encoder,
                        view,
                        canvas,
                        target_initialized,
                        "render group composite",
                    );
                    self.composites
                        .draw(&mut pass, std::slice::from_ref(&item), viewport);
                }
                queue_submit += self.submit(encoder);
                target_initialized = true;
            }

            if layer == PaintLayer::Content && canvas_has_text(canvas) {
                self.text.prepare(
                    &self.gpu.device,
                    &self.gpu.queue,
                    canvas,
                    viewport,
                    text_system,
                )?;
                let mut encoder = self.create_encoder("render text layer encoder");
                {
                    let mut pass = begin_drawing_pass(
                        &mut encoder,
                        view,
                        canvas,
                        target_initialized,
                        "render text layer",
                    );
                    self.text.draw(&mut pass)?;
                }
                queue_submit += self.submit(encoder);
                target_initialized = true;
            }
        }

        if !target_initialized {
            let mut encoder = self.create_encoder("render empty canvas encoder");
            drop(begin_drawing_pass(
                &mut encoder,
                view,
                canvas,
                false,
                "render empty canvas",
            ));
            queue_submit += self.submit(encoder);
        }
        self.text.finish_frame();
        Ok(queue_submit)
    }

    fn create_encoder(&self, label: &'static str) -> wgpu::CommandEncoder {
        self.gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    fn submit(&self, encoder: wgpu::CommandEncoder) -> Duration {
        let started_at = Instant::now();
        self.gpu.queue.submit([encoder.finish()]);
        started_at.elapsed()
    }
}

fn begin_drawing_pass<'encoder>(
    encoder: &'encoder mut wgpu::CommandEncoder,
    view: &'encoder wgpu::TextureView,
    canvas: &Canvas,
    initialized: bool,
    label: &'static str,
) -> wgpu::RenderPass<'encoder> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if initialized {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(canvas.clear_color.as_wgpu_clear())
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn canvas_has_shapes_in_layer(canvas: &Canvas, layer: PaintLayer) -> bool {
    canvas.commands().iter().any(|command| match command {
        DrawCommand::Decoration {
            layer: command_layer,
            ..
        } => *command_layer == layer,
        DrawCommand::Box { .. } => layer == PaintLayer::Block,
        DrawCommand::OverlayBox { .. } => layer == PaintLayer::Overlay,
        DrawCommand::Text { .. } | DrawCommand::Group { .. } => false,
    })
}

fn canvas_has_text(canvas: &Canvas) -> bool {
    canvas
        .commands()
        .iter()
        .any(|command| matches!(command, DrawCommand::Text { .. }))
}

fn group_can_draw_directly(
    canvas: &Canvas,
    transform: Transform,
    opacity: f32,
    clips: &[crate::Clip],
) -> bool {
    canvas.clear_color == Color::TRANSPARENT
        && transform == Transform::IDENTITY
        && opacity >= 1.0
        && clips.is_empty()
}

fn group_target_viewport(
    canvas: &Canvas,
    parent: TargetViewport,
    maximum_dimension: u32,
) -> Result<Option<TargetViewport>, RenderError> {
    let Some(bounds) = canvas_visual_bounds(canvas, parent) else {
        return Ok(None);
    };
    let left = bounds.x.floor();
    let top = bounds.y.floor();
    let right = (bounds.x + bounds.width).ceil();
    let bottom = (bounds.y + bounds.height).ceil();
    if ![left, top, right, bottom].into_iter().all(f32::is_finite) || right <= left || bottom <= top
    {
        return Ok(None);
    }
    let width = right - left;
    let height = bottom - top;
    if width > maximum_dimension as f32 || height > maximum_dimension as f32 {
        return Err(RenderError::GroupTargetTooLarge);
    }
    Ok(Some(TargetViewport {
        size: SurfaceSize::new(width as u32, height as u32),
        origin: [left, top],
    }))
}

fn canvas_visual_bounds(canvas: &Canvas, fallback: TargetViewport) -> Option<Rect> {
    let mut bounds = (canvas.clear_color != Color::TRANSPARENT).then(|| fallback.rect());
    for command in canvas.commands() {
        let command_bounds = match command {
            DrawCommand::Decoration {
                rect,
                decoration,
                style,
                ..
            } => decoration_bounds(*rect, decoration, *style),
            DrawCommand::Box { rect, style, .. } | DrawCommand::OverlayBox { rect, style, .. } => {
                box_bounds(*rect, style)
            }
            DrawCommand::Text {
                bounds,
                style,
                spans,
                ..
            } => {
                let mut visual = text_bounds(*bounds, style);
                for span in spans {
                    visual = union_rect(visual, text_bounds(*bounds, &span.style));
                }
                visual
            }
            DrawCommand::Group {
                canvas,
                origin,
                transform,
                clips,
                ..
            } => {
                let Some(child) = canvas_visual_bounds(canvas, fallback) else {
                    continue;
                };
                let mut visual = transformed_rect(child, *transform, *origin);
                for clip in clips {
                    visual = visual.intersection(clip.bounds());
                }
                visual
            }
        };
        if command_bounds.width > 0.0 && command_bounds.height > 0.0 {
            bounds = Some(bounds.map_or(command_bounds, |current| {
                union_rect(current, command_bounds)
            }));
        }
    }
    bounds
}

fn decoration_bounds(rect: Rect, decoration: &BoxDecoration, style: DecorationStyle) -> Rect {
    let visual = match decoration {
        BoxDecoration::Outline(outline) => {
            expand_rect(rect, (outline.offset + outline.width).max(0.0))
        }
        BoxDecoration::Shadow(shadow) if !shadow.inset => {
            let mut shadow_rect =
                expand_rect(rect, shadow.spread.max(0.0) + shadow.blur.max(0.0) * 2.0);
            shadow_rect.x += shadow.offset[0];
            shadow_rect.y += shadow.offset[1];
            shadow_rect
        }
        BoxDecoration::Background { .. } | BoxDecoration::Border(_) | BoxDecoration::Shadow(_) => {
            rect
        }
    };
    expand_rect(
        transformed_rect(visual, style.transform, rect_center(rect)),
        2.0,
    )
}

fn box_bounds(rect: Rect, style: &BoxStyle) -> Rect {
    let mut visual = rect;
    if let Some(outline) = style.outline {
        visual = union_rect(
            visual,
            expand_rect(rect, (outline.offset + outline.width).max(0.0)),
        );
    }
    for shadow in style.shadows.iter().filter(|shadow| !shadow.inset) {
        let mut shadow_rect =
            expand_rect(rect, shadow.spread.max(0.0) + shadow.blur.max(0.0) * 2.0);
        shadow_rect.x += shadow.offset[0];
        shadow_rect.y += shadow.offset[1];
        visual = union_rect(visual, shadow_rect);
    }
    expand_rect(
        transformed_rect(visual, style.transform, rect_center(rect)),
        2.0,
    )
}

fn text_bounds(bounds: Rect, style: &TextStyle) -> Rect {
    let transformed = transformed_rect(bounds, style.transform, rect_center(bounds));
    let mut visual = transformed;
    for shadow in &style.shadows {
        let mut shadow_rect = expand_rect(transformed, shadow.blur.max(0.0));
        shadow_rect.x += shadow.offset[0];
        shadow_rect.y += shadow.offset[1];
        visual = union_rect(visual, shadow_rect);
    }
    expand_rect(visual, 2.0)
}

fn rect_center(rect: Rect) -> [f32; 2] {
    [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5]
}

fn transformed_rect(rect: Rect, transform: Transform, origin: [f32; 2]) -> Rect {
    let [a, b, c, d, tx, ty] = transform.matrix;
    let corners = [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ];
    let transformed = corners.map(|point| {
        let relative = [point[0] - origin[0], point[1] - origin[1]];
        [
            origin[0] + a * relative[0] + c * relative[1] + tx,
            origin[1] + b * relative[0] + d * relative[1] + ty,
        ]
    });
    let min_x = transformed
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = transformed
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = transformed
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = transformed
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn expand_rect(rect: Rect, amount: f32) -> Rect {
    Rect::new(
        rect.x - amount,
        rect.y - amount,
        rect.width + amount * 2.0,
        rect.height + amount * 2.0,
    )
}

fn union_rect(left: Rect, right: Rect) -> Rect {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    let right_edge = (left.x + left.width).max(right.x + right.width);
    let bottom = (left.y + left.height).max(right.y + right.height);
    Rect::new(x, y, right_edge - x, bottom - y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackgroundImage, Border, BoxDecoration, BoxShadow, BoxStyle, Clip, Color, CornerRadius,
        DecorationStyle, GradientStop, Outline, PaintLayer, RasterImage, Rect, TextStyle,
        Transform,
    };

    #[tokio::test(flavor = "current_thread")]
    async fn renders_small_decorations_in_layer_order() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(32, 32),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut canvas = Canvas::new().with_clear_color(Color::WHITE);

        // Submit the front decoration first to prove paint layers, rather than
        // insertion order across layers, decide the final composition.
        canvas.draw_decoration(
            PaintLayer::Outline,
            Rect::new(8.0, 8.0, 16.0, 16.0),
            BoxDecoration::Background {
                color: Color::from_rgba8(0, 0, 255, 255),
                image: None,
            },
            DecorationStyle::default(),
        );
        canvas.draw_decoration(
            PaintLayer::ContextBackground,
            Rect::new(0.0, 0.0, 32.0, 32.0),
            BoxDecoration::Background {
                color: Color::from_rgba8(255, 0, 0, 255),
                image: None,
            },
            DecorationStyle::default(),
        );

        let image = readback::draw_to_image(
            &mut renderer,
            &canvas,
            SurfaceSize::new(32, 32),
            &mut text_system,
        )
        .expect("off-screen layered decoration render");
        let center = image.pixel(16, 16).unwrap();
        assert!(center[2] > 220 && center[0] < 40, "{center:?}");
        let edge = image.pixel(2, 2).unwrap();
        assert!(edge[0] > 220 && edge[2] < 40, "{edge:?}");
        drop(adapter);
    }

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

    #[tokio::test(flavor = "current_thread")]
    async fn deeply_nested_groups_are_not_silently_omitted() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok((gpu, adapter)) = Gpu::new(&instance, None).await else {
            return;
        };
        let surface = SurfaceState::offscreen(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            SurfaceSize::new(32, 32),
        );
        let mut renderer = Renderer::from_gpu(gpu, surface);
        let mut text_system = TextSystem::new();
        let mut nested = Canvas::new();
        nested.draw_box(
            Rect::new(8.0, 8.0, 16.0, 16.0),
            BoxStyle {
                background: Color::from_rgba8(220, 30, 40, 255),
                ..BoxStyle::default()
            },
        );
        for _ in 0..40 {
            let mut parent = Canvas::new();
            parent.draw_group(nested, [16.0, 16.0], Transform::IDENTITY, 0.999, []);
            nested = parent;
        }
        nested.clear_color = Color::WHITE;

        let image = readback::draw_to_image(
            &mut renderer,
            &nested,
            SurfaceSize::new(32, 32),
            &mut text_system,
        )
        .expect("deep stacking groups");

        let center = image.pixel(16, 16).unwrap();
        assert!(center[0] > 180 && center[1] < 80 && center[2] < 90);
        drop(adapter);
    }

    #[test]
    fn group_targets_are_cropped_to_their_content() {
        let mut group = Canvas::new();
        group.draw_box(
            Rect::new(10.0, 20.0, 30.0, 40.0),
            BoxStyle {
                background: Color::WHITE,
                ..BoxStyle::default()
            },
        );

        let viewport = group_target_viewport(
            &group,
            TargetViewport::surface(SurfaceSize::new(1920, 1080)),
            8192,
        )
        .unwrap()
        .unwrap();

        assert_eq!(viewport.origin, [8.0, 18.0]);
        assert_eq!(viewport.size, SurfaceSize::new(34, 44));
    }

    #[test]
    fn empty_transparent_groups_do_not_allocate_targets() {
        let viewport = group_target_viewport(
            &Canvas::new(),
            TargetViewport::surface(SurfaceSize::new(1920, 1080)),
            8192,
        )
        .unwrap();

        assert!(viewport.is_none());
    }

    #[test]
    fn plain_stacking_groups_can_draw_without_an_offscreen_target() {
        let group = Canvas::new();
        assert!(group_can_draw_directly(
            &group,
            Transform::IDENTITY,
            1.0,
            &[],
        ));
        assert!(!group_can_draw_directly(
            &group,
            Transform::IDENTITY,
            0.5,
            &[],
        ));
        assert!(!group_can_draw_directly(
            &group,
            Transform {
                matrix: [1.0, 0.0, 0.0, 1.0, 2.0, 0.0],
            },
            1.0,
            &[],
        ));
    }
}
