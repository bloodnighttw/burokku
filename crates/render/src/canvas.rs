//! A window-backed canvas and its retained frame command list.

use std::ops::{Deref, DerefMut};

use crate::{
    backdrop::{BackdropDraw, BackdropPayload, BackdropRendererHandle},
    engine::{RenderEngine, RenderEngineError, RenderTarget},
    raster::{RasterPayload, RasterRendererHandle, RendererDraw},
    shapes::{rect::Rect, round::Round, stroke::Stroke},
};

/// One backend-independent drawing operation.
///
/// Commands are retained in submission order. Built-in geometry keeps an
/// explicit representation while custom raster renderers use [`Self::Raster`].
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Rect {
        rect: Rect,
        round: Round,
        color: wgpu::Color,
    },
    /// Restricts following commands to `rect` until the matching [`Self::PopClip`].
    PushClip { rect: Rect, round: Round },
    /// Restores the clip active before the matching [`Self::PushClip`].
    PopClip,
    Stroke {
        stroke: Stroke,
        round: Round,
        color: wgpu::Color,
    },
    /// A payload recorded for a typed [`RasterRendererHandle`].
    Raster(RendererDraw),
    /// A typed effect that samples the scene produced by earlier commands.
    Backdrop(BackdropDraw),
}

impl DrawCommand {
    pub const fn rect(rect: Rect, color: wgpu::Color, round: Round) -> Self {
        Self::Rect { rect, color, round }
    }

    pub const fn stroke(stroke: Stroke, color: wgpu::Color, round: Round) -> Self {
        Self::Stroke {
            stroke,
            round,
            color,
        }
    }

    pub const fn push_clip(rect: Rect, round: Round) -> Self {
        Self::PushClip { rect, round }
    }

    pub const fn pop_clip() -> Self {
        Self::PopClip
    }
}

/// A mockable, GPU-independent list of drawing operations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    commands: Vec<DrawCommand>,
}

impl DrawList {
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn draw(&mut self, command: impl Into<DrawCommand>) -> &mut Self {
        self.commands.push(command.into());
        self
    }

    /// Records `payload` for a reusable typed raster renderer registration.
    pub fn draw_with<P: RasterPayload>(
        &mut self,
        renderer: &RasterRendererHandle<P>,
        payload: P,
    ) -> &mut Self {
        self.draw(renderer.command(payload))
    }

    /// Records a typed effect that samples the scene drawn so far.
    pub fn backdrop_with<P: BackdropPayload>(
        &mut self,
        renderer: &BackdropRendererHandle<P>,
        payload: P,
    ) -> &mut Self {
        self.draw(renderer.command(payload))
    }

    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Begins a nested rectangular clip scope.
    pub fn push_clip(&mut self, rect: Rect) -> &mut Self {
        self.push_rounded_clip(rect, Round::default())
    }

    /// Begins a nested rounded-rectangle clip scope.
    pub fn push_rounded_clip(&mut self, rect: Rect, round: Round) -> &mut Self {
        self.draw(DrawCommand::push_clip(rect, round))
    }

    /// Ends the most recently started clip scope.
    pub fn pop_clip(&mut self) -> &mut Self {
        self.draw(DrawCommand::pop_clip())
    }

    /// Records a balanced rectangular clip around `draw`.
    pub fn with_clip<R>(&mut self, rect: Rect, draw: impl FnOnce(&mut Self) -> R) -> R {
        self.push_clip(rect);
        let output = draw(self);
        self.pop_clip();
        output
    }

    /// Records a balanced rounded-rectangle clip around `draw`.
    pub fn with_rounded_clip<R>(
        &mut self,
        rect: Rect,
        round: Round,
        draw: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.push_rounded_clip(rect, round);
        let output = draw(self);
        self.pop_clip();
        output
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

/// Errors produced while configuring or resizing a canvas surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CanvasSurfaceError {
    #[error("the adapter does not support this surface")]
    UnsupportedSurface,
    #[error("a surface cannot be configured with a zero width or height")]
    ZeroSize,
    #[error("the canvas surface and GPU must be configured before it can be resized")]
    NotConfigured,
}

/// Errors produced while presenting a canvas frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CanvasRenderError {
    #[error("the canvas surface and GPU must be configured before drawing")]
    NotConfigured,
    #[error("timed out while acquiring the next surface texture")]
    SurfaceTimeout,
    #[error("the canvas surface is occluded")]
    SurfaceOccluded,
    #[error("the canvas surface is outdated and must be configured again")]
    SurfaceOutdated,
    #[error("the canvas surface was lost and must be recreated")]
    SurfaceLost,
    #[error("surface texture acquisition failed validation")]
    SurfaceValidation,
    #[error("draw commands contain an unmatched push_clip or pop_clip")]
    UnbalancedClipStack,
}

/// A surface-backed drawing canvas.
///
/// Canvas owns the shared device and queue used by every primitive renderer.
/// Its coordinate system starts at the top-left corner, at `(0, 0)`.
pub struct Canvas<'window> {
    surface: wgpu::Surface<'window>,
    config: Option<wgpu::SurfaceConfiguration>,
    engine: Option<RenderEngine>,
}

impl<'window> Canvas<'window> {
    /// Creates an unconfigured canvas backed by `target`.
    ///
    /// Passing an owned window (for example, an `Arc<Window>`) lets wgpu keep
    /// the window alive for as long as the canvas exists.
    pub fn new(
        instance: &wgpu::Instance,
        target: impl Into<wgpu::SurfaceTarget<'window>>,
    ) -> Result<Self, wgpu::CreateSurfaceError> {
        Ok(Self::from_surface(instance.create_surface(target)?))
    }

    /// Wraps an already-created WebGPU surface in an unconfigured canvas.
    pub fn from_surface(surface: wgpu::Surface<'window>) -> Self {
        Self {
            surface,
            config: None,
            engine: None,
        }
    }

    pub fn surface(&self) -> &wgpu::Surface<'window> {
        &self.surface
    }

    pub fn device(&self) -> Option<&wgpu::Device> {
        self.engine.as_ref().map(RenderEngine::device)
    }

    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.engine.as_ref().map(RenderEngine::queue)
    }

    pub fn configuration(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.config.as_ref()
    }

    /// Configures the surface and installs shared GPU state.
    ///
    /// The device and queue handles are cheap clones of wgpu's reference-
    /// counted handles. Primitive renderers remain private Canvas resources.
    pub fn configure(
        &mut self,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<(), CanvasSurfaceError> {
        if width == 0 || height == 0 {
            return Err(CanvasSurfaceError::ZeroSize);
        }

        let config = self
            .surface
            .get_default_config(adapter, width, height)
            .ok_or(CanvasSurfaceError::UnsupportedSurface)?;
        self.surface.configure(device, &config);

        self.engine = Some(RenderEngine::new(device, queue, config.format, 1));
        self.config = Some(config);
        Ok(())
    }

    /// Changes the configured surface size to match the window.
    ///
    /// Zero-sized events are ignored because wgpu surfaces cannot be
    /// configured with a zero dimension.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), CanvasSurfaceError> {
        if width == 0 || height == 0 {
            return Ok(());
        }

        let device = self
            .engine
            .as_ref()
            .map(RenderEngine::device)
            .ok_or(CanvasSurfaceError::NotConfigured)?;
        let config = self
            .config
            .as_mut()
            .ok_or(CanvasSurfaceError::NotConfigured)?;
        if config.width == width && config.height == height {
            return Ok(());
        }

        config.width = width;
        config.height = height;
        self.surface.configure(device, config);
        Ok(())
    }

    pub fn size(&self) -> Option<(u32, u32)> {
        self.config
            .as_ref()
            .map(|config| (config.width, config.height))
    }

    /// Starts recording a frame without performing GPU work.
    pub fn begin_frame(&mut self, clear_color: wgpu::Color) -> Frame<'_, 'window> {
        Frame {
            canvas: self,
            clear_color,
            draws: DrawList::new(),
        }
    }

    pub fn into_surface(self) -> wgpu::Surface<'window> {
        self.surface
    }

    fn render(
        &mut self,
        draws: &DrawList,
        clear_color: wgpu::Color,
        pre_present: impl FnOnce(),
    ) -> Result<(), CanvasRenderError> {
        let config = self
            .config
            .as_ref()
            .ok_or(CanvasRenderError::NotConfigured)?;
        let engine = self
            .engine
            .as_mut()
            .ok_or(CanvasRenderError::NotConfigured)?;
        engine
            .prepare(draws, [config.width, config.height])
            .map_err(|error| match error {
                RenderEngineError::UnbalancedClipStack => CanvasRenderError::UnbalancedClipStack,
            })?;

        let surface_frame = acquire_frame(&self.surface)?;
        let view = surface_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let commands = engine.encode(
            RenderTarget {
                color_view: &view,
                resolve_view: None,
                store: wgpu::StoreOp::Store,
            },
            clear_color,
        );

        engine.queue().submit([commands]);
        pre_present();
        surface_frame.present();
        Ok(())
    }
}

/// A retained frame that submits all commands in one wgpu frame lifecycle.
pub struct Frame<'canvas, 'window> {
    canvas: &'canvas mut Canvas<'window>,
    clear_color: wgpu::Color,
    draws: DrawList,
}

impl Frame<'_, '_> {
    pub fn present(self) -> Result<(), CanvasRenderError> {
        self.present_with_pre_present(|| {})
    }

    pub fn present_with_pre_present(
        self,
        pre_present: impl FnOnce(),
    ) -> Result<(), CanvasRenderError> {
        let Self {
            canvas,
            clear_color,
            draws,
        } = self;
        canvas.render(&draws, clear_color, pre_present)
    }
}

impl Deref for Frame<'_, '_> {
    type Target = DrawList;

    fn deref(&self) -> &Self::Target {
        &self.draws
    }
}

impl DerefMut for Frame<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.draws
    }
}

fn acquire_frame(surface: &wgpu::Surface<'_>) -> Result<wgpu::SurfaceTexture, CanvasRenderError> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
        wgpu::CurrentSurfaceTexture::Timeout => Err(CanvasRenderError::SurfaceTimeout),
        wgpu::CurrentSurfaceTexture::Occluded => Err(CanvasRenderError::SurfaceOccluded),
        wgpu::CurrentSurfaceTexture::Outdated => Err(CanvasRenderError::SurfaceOutdated),
        wgpu::CurrentSurfaceTexture::Lost => Err(CanvasRenderError::SurfaceLost),
        wgpu::CurrentSurfaceTexture::Validation => Err(CanvasRenderError::SurfaceValidation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offscreen::OffscreenSurface;

    #[test]
    fn draw_list_retains_submission_order_without_a_gpu() {
        let first = DrawCommand::rect(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            wgpu::Color::RED,
            Round::default(),
        );
        let second = DrawCommand::stroke(
            Stroke::new(5.0, 5.0, 20.0, 20.0, 2.0),
            wgpu::Color::BLUE,
            Round::default(),
        );
        let mut draws = DrawList::new();

        draws.draw(first.clone()).draw(second.clone());

        assert_eq!(draws.commands(), &[first, second]);
    }

    #[test]
    fn scoped_clip_records_balanced_commands_in_order() {
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        let child = DrawCommand::rect(
            Rect::new(90.0, 90.0, 20.0, 20.0),
            wgpu::Color::RED,
            Round::default(),
        );
        let mut draws = DrawList::new();

        draws.with_clip(clip, |draws| {
            draws.draw(child.clone());
        });

        assert_eq!(
            draws.commands(),
            &[
                DrawCommand::PushClip {
                    rect: clip,
                    round: Round::default()
                },
                child,
                DrawCommand::PopClip,
            ]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_canvas_renders_a_stroke_command() {
        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping offscreen stroke test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws.draw(DrawCommand::stroke(
            Stroke::new(2.0, 2.0, 12.0, 12.0, 2.0),
            wgpu::Color::RED,
            Round::default(),
        ));

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 8, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 8), [0, 0, 255, 255]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn unified_shape_pipeline_preserves_command_order() {
        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping shape order test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws
            .draw(DrawCommand::rect(
                Rect::new(0.0, 0.0, 16.0, 16.0),
                wgpu::Color::RED,
                Round::default(),
            ))
            .draw(DrawCommand::stroke(
                Stroke::new(2.0, 2.0, 12.0, 12.0, 6.0),
                wgpu::Color::BLUE,
                Round::default(),
            ))
            .draw(DrawCommand::rect(
                Rect::new(6.0, 6.0, 4.0, 4.0),
                wgpu::Color::GREEN,
                Round::default(),
            ));

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLACK).await;

        assert_eq!(surface.pixel(&pixels, 0, 0), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 3, 3), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 8), [0, 255, 0, 255]);
    }
}
