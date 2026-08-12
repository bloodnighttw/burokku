//! Render targets and draw-command encoding.

use crate::{
    engine::Engine,
    attributes::{corner::Corner, rect::Rect},
};

/// A drawing operation recorded for a canvas.
///
/// The primitive variants are intentionally small while their payload types
/// are being built in `variants`. Command interpretation belongs in this
/// module so every kind of canvas has identical drawing behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawCommand {
    PushClip {
        rect: Rect,
        corners: Corner,
    },
    PopClip,
    // fill an area with rect and corners
    Fill {
        // we don't need to store corners as it will be covered by
        // push clip and pop clip
        rect: Rect,
        color: wgpu::Color,
    },
    // stroke an area with rect and corners
    Stroke {
        rect: Rect,
        // the width of the stroke, note this is not same as the rect's dimensions
        // and it is drawn inside the rect
        width: f32,
        corners: Corner,
        color: wgpu::Color,
    },
}

/// Errors that can occur while creating or resizing a canvas.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CanvasError {
    #[error("a canvas must have a non-zero width and height")]
    ZeroSize,
    #[error("the adapter does not support this canvas surface")]
    UnsupportedSurface,
    #[error("the canvas must be configured before it can be resized")]
    NotConfigured,
}

/// Errors that can occur while drawing a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DrawError {
    #[error("the canvas must be configured before drawing")]
    NotConfigured,
    #[error("draw commands contain an unmatched push or pop clip")]
    UnbalancedClipStack,
    #[error("timed out while acquiring the next surface texture")]
    SurfaceTimeout,
    #[error("the surface is occluded")]
    SurfaceOccluded,
    #[error("the surface is outdated and must be configured again")]
    SurfaceOutdated,
    #[error("the surface was lost and must be recreated")]
    SurfaceLost,
    #[error("surface texture acquisition failed validation")]
    SurfaceValidation,
}

/// A window- or display-backed canvas.
///
/// The canvas owns its surface and a format-compatible [`Engine`]. Its
/// coordinate system starts at the top-left corner, at `(0, 0)`.
pub struct Canvas<'surface> {
    surface: wgpu::Surface<'surface>,
    config: Option<wgpu::SurfaceConfiguration>,
    engine: Option<Engine>,
}

impl<'surface> Canvas<'surface> {
    /// Creates an unconfigured canvas from a window or another surface target.
    ///
    /// Passing an owned target such as `Arc<Window>` lets wgpu retain the
    /// window for the lifetime of the returned surface:
    ///
    /// ```ignore
    /// let mut canvas = Canvas::new(&instance, window.clone())?;
    /// let size = window.inner_size();
    /// canvas.configure(&adapter, &device, &queue, size.width, size.height)?;
    /// ```
    pub fn new(
        instance: &wgpu::Instance,
        target: impl Into<wgpu::SurfaceTarget<'surface>>,
    ) -> Result<Self, wgpu::CreateSurfaceError> {
        Ok(Self::from_surface(instance.create_surface(target)?))
    }

    /// Wraps an already-created surface.
    pub fn from_surface(surface: wgpu::Surface<'surface>) -> Self {
        Self {
            surface,
            config: None,
            engine: None,
        }
    }

    pub fn surface(&self) -> &wgpu::Surface<'surface> {
        &self.surface
    }

    pub fn configuration(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.config.as_ref()
    }

    pub fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }

    pub fn device(&self) -> Option<&wgpu::Device> {
        self.engine.as_ref().map(Engine::device)
    }

    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.engine.as_ref().map(Engine::queue)
    }

    pub fn size(&self) -> Option<[u32; 2]> {
        self.config
            .as_ref()
            .map(|config| [config.width, config.height])
    }

    /// Chooses a surface configuration compatible with `adapter` and installs
    /// it using the engine's device.
    pub fn configure(
        &mut self,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<(), CanvasError> {
        validate_size(width, height)?;
        let config = self
            .surface
            .get_default_config(adapter, width, height)
            .ok_or(CanvasError::UnsupportedSurface)?;
        self.surface.configure(device, &config);
        self.engine = Some(Engine::new(device, queue));
        self.config = Some(config);
        Ok(())
    }

    /// Reconfigures the surface for a new non-zero size.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), CanvasError> {
        validate_size(width, height)?;
        let device = self
            .engine
            .as_ref()
            .map(Engine::device)
            .ok_or(CanvasError::NotConfigured)?;
        let config = self.config.as_mut().ok_or(CanvasError::NotConfigured)?;
        if config.width == width && config.height == height {
            return Ok(());
        }

        config.width = width;
        config.height = height;
        self.surface.configure(device, config);
        Ok(())
    }

    /// Encodes, submits, and presents one frame.
    pub fn draw(
        &mut self,
        commands: &[DrawCommand],
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex, DrawError> {
        self.config.as_ref().ok_or(DrawError::NotConfigured)?;
        let engine = self.engine.as_mut().ok_or(DrawError::NotConfigured)?;
        validate_clip_stack(commands)?;
        let frame = acquire_surface_frame(&self.surface)?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let command_buffer = encode_draw_commands(engine, &view, commands, clear_color)?;
        let submission = engine.submit(command_buffer);
        frame.present();
        Ok(submission)
    }

    pub fn into_surface(self) -> wgpu::Surface<'surface> {
        self.surface
    }
}

/// A texture-backed canvas for tests, image generation, and composition.
///
/// It uses the exact same command encoder as [`Canvas`] but does not acquire or
/// present a swapchain image.
pub struct OffscreenCanvas {
    engine: Engine,
    texture: wgpu::Texture,
    size: [u32; 2],
}

impl OffscreenCanvas {
    pub fn new(engine: Engine, width: u32, height: u32) -> Result<Self, CanvasError> {
        validate_size(width, height)?;
        Ok(Self::create(engine, [width, height]))
    }

    /// Returns the single-sampled RGBA8 texture containing the rendered result.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub const fn size(&self) -> [u32; 2] {
        self.size
    }

    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), CanvasError> {
        validate_size(width, height)?;
        if self.size == [width, height] {
            return Ok(());
        }

        self.texture = create_offscreen_texture(&self.engine, [width, height]);
        self.size = [width, height];
        Ok(())
    }

    /// Encodes and submits one frame into the backing texture.
    pub fn draw(
        &mut self,
        commands: &[DrawCommand],
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex, DrawError> {
        let engine = &mut self.engine;
        let view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let command_buffer = encode_draw_commands(engine, &view, commands, clear_color)?;
        Ok(engine.submit(command_buffer))
    }

    fn create(engine: Engine, size: [u32; 2]) -> Self {
        let texture = create_offscreen_texture(&engine, size);
        Self {
            engine,
            texture,
            size,
        }
    }
}

fn create_offscreen_texture(engine: &Engine, size: [u32; 2]) -> wgpu::Texture {
    engine.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("render offscreen canvas"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// The single draw path shared by every render target.
///
/// Pipeline and buffer selection for `Fill` and `Stroke` will be added here as
/// their command payloads are introduced. Keeping the render pass here avoids
/// teaching `Engine` about surface or texture ownership.
fn encode_draw_commands(
    engine: &Engine,
    target: &wgpu::TextureView,
    commands: &[DrawCommand],
    clear_color: wgpu::Color,
) -> Result<wgpu::CommandBuffer, DrawError> {
    validate_clip_stack(commands)?;

    let mut encoder = engine.create_command_encoder(Some("render canvas encoder"));
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render canvas pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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

        for command in commands {
            match command {
                DrawCommand::PushClip { .. } | DrawCommand::PopClip => {}
                DrawCommand::Fill { .. } => draw_fill(&mut pass),
                DrawCommand::Stroke { .. } => draw_stroke(&mut pass),
            }
        }
    }
    Ok(encoder.finish())
}

fn draw_fill(_pass: &mut wgpu::RenderPass<'_>) {
    // The fill pipeline is installed here once `DrawCommand::Fill` carries a
    // block payload.
}

fn draw_stroke(_pass: &mut wgpu::RenderPass<'_>) {
    // The stroke pipeline is installed here once `DrawCommand::Stroke` carries
    // a stroke payload.
}

fn validate_size(width: u32, height: u32) -> Result<(), CanvasError> {
    if width == 0 || height == 0 {
        Err(CanvasError::ZeroSize)
    } else {
        Ok(())
    }
}

fn validate_clip_stack(commands: &[DrawCommand]) -> Result<(), DrawError> {
    let mut depth = 0usize;
    for command in commands {
        match command {
            DrawCommand::PushClip { .. } => depth += 1,
            DrawCommand::PopClip if depth == 0 => return Err(DrawError::UnbalancedClipStack),
            DrawCommand::PopClip => depth -= 1,
            DrawCommand::Fill { .. } | DrawCommand::Stroke { .. } => {}
        }
    }

    if depth == 0 {
        Ok(())
    } else {
        Err(DrawError::UnbalancedClipStack)
    }
}

fn acquire_surface_frame(surface: &wgpu::Surface<'_>) -> Result<wgpu::SurfaceTexture, DrawError> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(frame),
        wgpu::CurrentSurfaceTexture::Timeout => Err(DrawError::SurfaceTimeout),
        wgpu::CurrentSurfaceTexture::Occluded => Err(DrawError::SurfaceOccluded),
        wgpu::CurrentSurfaceTexture::Outdated => Err(DrawError::SurfaceOutdated),
        wgpu::CurrentSurfaceTexture::Lost => Err(DrawError::SurfaceLost),
        wgpu::CurrentSurfaceTexture::Validation => Err(DrawError::SurfaceValidation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_sized_canvases() {
        assert_eq!(validate_size(0, 10), Err(CanvasError::ZeroSize));
        assert_eq!(validate_size(10, 0), Err(CanvasError::ZeroSize));
    }

    #[test]
    fn accepts_balanced_nested_clips() {
        let commands = [
            push_clip(),
            DrawCommand::Fill {
                rect: Rect::default(),
                corners: Corner::default(),
                color: wgpu::Color::BLACK,
            },
            push_clip(),
            DrawCommand::Stroke {
                rect: Rect::default(),
                width: 1.0,
                corners: Corner::default(),
                color: wgpu::Color::BLACK,
            },
            DrawCommand::PopClip,
            DrawCommand::PopClip,
        ];

        assert_eq!(validate_clip_stack(&commands), Ok(()));
    }

    #[test]
    fn rejects_unbalanced_clips() {
        assert_eq!(
            validate_clip_stack(&[DrawCommand::PopClip]),
            Err(DrawError::UnbalancedClipStack)
        );
        assert_eq!(
            validate_clip_stack(&[push_clip()]),
            Err(DrawError::UnbalancedClipStack)
        );
    }

    fn push_clip() -> DrawCommand {
        DrawCommand::PushClip {
            rect: Rect::default(),
            corners: Corner::default(),
        }
    }
}
