//! Target-independent frame preparation and GPU command encoding.

use std::ops::Range;

use crate::{
    backdrop::{BackdropSystem, ScheduledBackdrop},
    canvas::{DrawCommand, DrawList},
    clip::ClipResolver,
    compositor::SceneCompositor,
    raster::RasterSystem,
    shapes::rounded_rect::{rounded_rect_handle, RoundedRectDraw},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderEngineError {
    UnbalancedClipStack,
}

/// The output attachment used for one encoded frame.
///
/// Window and offscreen targets own their textures and only lend their views to
/// the engine. Keeping target acquisition outside the engine lets the same
/// rendering lifecycle serve swapchain images, multisampled textures, and
/// future compositor-owned scene textures.
pub(crate) struct RenderTarget<'view> {
    pub(crate) color_view: &'view wgpu::TextureView,
    pub(crate) resolve_view: Option<&'view wgpu::TextureView>,
    pub(crate) store: wgpu::StoreOp,
}

/// Persistent target-independent GPU renderer state.
///
/// Target adapters retain responsibility for acquiring and presenting images
/// or reading textures back. The engine owns shared GPU handles, prepares the
/// retained draw list, and encodes the render passes common to every target.
pub(crate) struct RenderEngine {
    device: wgpu::Device,
    queue: wgpu::Queue,
    compositor: SceneCompositor,
    raster: RasterSystem,
    backdrop: BackdropSystem,
    operations: Vec<SceneOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SceneOperation {
    Raster(Range<usize>),
    Backdrop(ScheduledBackdrop),
}

impl RenderEngine {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let compositor = SceneCompositor::new(device, format, sample_count);
        let backdrop =
            BackdropSystem::new(device, queue, format, compositor.scene_bind_group_layout());
        Self {
            device: device.clone(),
            queue: queue.clone(),
            compositor,
            raster: RasterSystem::new(device, queue, format, sample_count),
            backdrop,
            operations: Vec::new(),
        }
    }

    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Validates and uploads a retained frame before a target image is acquired.
    pub(crate) fn prepare(
        &mut self,
        draws: &DrawList,
        size: [u32; 2],
    ) -> Result<(), RenderEngineError> {
        self.compositor.ensure_size(&self.device, size);
        self.raster.begin_frame();
        self.backdrop.begin_frame();
        self.operations.clear();
        let mut clips = ClipResolver::new(size);
        for command in draws.commands() {
            match command {
                DrawCommand::PushClip { rect, round } => clips.push(*rect, *round),
                DrawCommand::PopClip => clips
                    .pop()
                    .map_err(|_| RenderEngineError::UnbalancedClipStack)?,
                DrawCommand::Rect { rect, round, color } => {
                    let clip = clips.current();
                    let batch = self.raster.queue(
                        rounded_rect_handle().renderer_draw(RoundedRectDraw::Fill {
                            rect: *rect,
                            color: *color,
                            round: *round,
                        }),
                        clip.scissor,
                        clip.masks,
                    );
                    self.record_raster_batch(batch);
                }
                DrawCommand::Stroke {
                    stroke,
                    round,
                    color,
                } => {
                    let clip = clips.current();
                    let batch = self.raster.queue(
                        rounded_rect_handle().renderer_draw(RoundedRectDraw::Stroke {
                            stroke: *stroke,
                            color: *color,
                            round: *round,
                        }),
                        clip.scissor,
                        clip.masks,
                    );
                    self.record_raster_batch(batch);
                }
                DrawCommand::Raster(draw) => {
                    let clip = clips.current();
                    let batch = self.raster.queue(draw.clone(), clip.scissor, clip.masks);
                    self.record_raster_batch(batch);
                }
                DrawCommand::Backdrop(draw) => {
                    let clip = clips.current();
                    if let Some(scheduled) =
                        self.backdrop.queue(draw.clone(), clip.scissor, clip.masks)
                    {
                        self.raster.break_batch();
                        self.operations.push(SceneOperation::Backdrop(scheduled));
                    }
                }
            }
        }
        let clip_masks = clips
            .finish()
            .map_err(|_| RenderEngineError::UnbalancedClipStack)?;
        self.raster.prepare(size, &clip_masks);
        self.backdrop.prepare(size, &clip_masks);
        Ok(())
    }

    /// Encodes the prepared frame without submitting it.
    ///
    /// Submission remains with the target adapter so it can order presentation,
    /// readback, and any target-specific command buffers around the rendered
    /// frame.
    pub(crate) fn encode(
        &self,
        target: RenderTarget<'_>,
        clear_color: wgpu::Color,
    ) -> wgpu::CommandBuffer {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render engine frame encoder"),
            });
        {
            self.compositor.clear_scene(&mut encoder, 0, clear_color);
        }

        let mut current_scene = 0usize;
        for operation in &self.operations {
            match operation {
                SceneOperation::Raster(batches) => {
                    let destination = if self.compositor.sample_count() > 1 {
                        1 - current_scene
                    } else {
                        current_scene
                    };
                    {
                        let mut pass = self.compositor.begin_raster_run(
                            &mut encoder,
                            current_scene,
                            destination,
                        );
                        self.raster.draw_range(&mut pass, batches.clone());
                    }
                    current_scene = destination;
                }
                SceneOperation::Backdrop(scheduled) => {
                    let destination = 1 - current_scene;
                    let source_bind_group = self.compositor.scene_source_bind_group(current_scene);
                    let effect_source =
                        self.backdrop
                            .encode_source(&mut encoder, *scheduled, source_bind_group);
                    {
                        let mut pass = self.compositor.begin_backdrop(
                            &mut encoder,
                            current_scene,
                            destination,
                        );
                        self.backdrop.draw(&mut pass, *scheduled, effect_source);
                    }
                    current_scene = destination;
                }
            }
        }
        self.compositor.present(&mut encoder, current_scene, target);

        encoder.finish()
    }

    fn record_raster_batch(&mut self, batch: Option<usize>) {
        let Some(batch) = batch else {
            return;
        };
        match self.operations.last_mut() {
            Some(SceneOperation::Raster(batches)) if batches.end == batch => {
                batches.end += 1;
            }
            Some(SceneOperation::Raster(batches)) if batches.end == batch + 1 => {}
            _ => self
                .operations
                .push(SceneOperation::Raster(batch..batch + 1)),
        }
    }
}
