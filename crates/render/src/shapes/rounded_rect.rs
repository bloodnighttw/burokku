//! Shared fill and stroke renderer for rounded rectangles.

use std::{ops::Range, sync::OnceLock};

use bytemuck::{Pod, Zeroable};

use crate::{
    clip::ClipMask,
    raster::{
        ClipMaskRange, RasterBatch, RasterCreateContext, RasterPrepareContext, RasterRenderer,
        RasterRendererFactory, RasterRendererHandle, ResolvedRasterDraw,
    },
    shapes::{rect::Rect, round::Round, stroke::Stroke},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RoundedRectDraw {
    Fill {
        rect: Rect,
        color: wgpu::Color,
        round: Round,
    },
    Stroke {
        stroke: Stroke,
        color: wgpu::Color,
        round: Round,
    },
}

pub(crate) fn rounded_rect_handle() -> &'static RasterRendererHandle<RoundedRectDraw> {
    static HANDLE: OnceLock<RasterRendererHandle<RoundedRectDraw>> = OnceLock::new();
    HANDLE
        .get_or_init(|| RasterRendererHandle::new("rounded rectangle", RoundedRectRendererFactory))
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RoundedRectRendererFactory;

impl RasterRendererFactory<RoundedRectDraw> for RoundedRectRendererFactory {
    type Renderer = RoundedRectRenderer;

    fn create(&self, context: RasterCreateContext<'_>) -> Self::Renderer {
        RoundedRectRenderer::new(context)
    }
}

pub(crate) struct RoundedRectRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    clip_buffer: wgpu::Buffer,
    clip_capacity: u64,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<RoundedRectInstance>,
    batches: Vec<Range<u32>>,
}

impl RoundedRectRenderer {
    fn new(context: RasterCreateContext<'_>) -> Self {
        let RasterCreateContext {
            device,
            queue: _,
            target_format,
            sample_count,
        } = context;
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render rounded rectangle screen uniform"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render rounded rectangle bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let clip_capacity = std::mem::size_of::<ClipMask>() as u64;
        let clip_buffer = create_clip_buffer(device, clip_capacity);
        let screen_bind_group =
            create_bind_group(device, &bind_group_layout, &screen_buffer, &clip_buffer);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render rounded rectangle shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("wgsl/rounded_rect.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render rounded rectangle pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let targets = [Some(wgpu::ColorTargetState {
            format: target_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render rounded rectangle pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[RoundedRectInstance::layout()],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: sample_count,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let instance_capacity = std::mem::size_of::<RoundedRectInstance>() as u64;
        let instance_buffer = create_instance_buffer(device, instance_capacity);

        Self {
            pipeline,
            bind_group_layout,
            screen_buffer,
            screen_bind_group,
            clip_buffer,
            clip_capacity,
            instance_buffer,
            instance_capacity,
            instances: Vec::new(),
            batches: Vec::new(),
        }
    }
}

impl RasterRenderer<RoundedRectDraw> for RoundedRectRenderer {
    fn prepare(
        &mut self,
        context: RasterPrepareContext<'_>,
        draws: &[ResolvedRasterDraw<'_, RoundedRectDraw>],
        batches: &[RasterBatch],
    ) {
        let RasterPrepareContext {
            device,
            queue,
            canvas_size,
            clip_masks,
        } = context;
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: [canvas_size[0] as f32, canvas_size[1] as f32],
                _padding: [0.0; 2],
            }),
        );

        self.instances.clear();
        self.batches.clear();
        collect_instances(draws, batches, &mut self.instances, &mut self.batches);

        let required_clips = std::mem::size_of_val(clip_masks) as u64;
        if required_clips > self.clip_capacity {
            self.clip_capacity = required_clips.next_power_of_two();
            self.clip_buffer = create_clip_buffer(device, self.clip_capacity);
            self.screen_bind_group = create_bind_group(
                device,
                &self.bind_group_layout,
                &self.screen_buffer,
                &self.clip_buffer,
            );
        }
        if !clip_masks.is_empty() {
            queue.write_buffer(&self.clip_buffer, 0, bytemuck::cast_slice(clip_masks));
        }

        if self.instances.is_empty() {
            return;
        }

        let required_instances = std::mem::size_of_val(self.instances.as_slice()) as u64;
        if required_instances > self.instance_capacity {
            self.instance_capacity = required_instances.next_power_of_two();
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
        }
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }

    fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, batch_index: usize) {
        let instances = &self.batches[batch_index];
        if instances.is_empty() {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..6, instances.clone());
    }
}

fn collect_instances(
    draws: &[ResolvedRasterDraw<'_, RoundedRectDraw>],
    batches: &[RasterBatch],
    instances: &mut Vec<RoundedRectInstance>,
    renderer_batches: &mut Vec<Range<u32>>,
) {
    for batch in batches {
        let first_instance = instances.len() as u32;
        for draw_index in batch.draws.clone() {
            let draw = &draws[draw_index];
            if let Some(instance) = RoundedRectInstance::from_draw(draw.payload, draw.clip_masks) {
                instances.push(instance);
            }
        }
        renderer_batches.push(first_instance..instances.len() as u32);
    }
}

fn create_instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render rounded rectangle instance buffer"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_clip_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render rounded clip buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    screen_buffer: &wgpu::Buffer,
    clip_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("render rounded rectangle bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: clip_buffer.as_entire_binding(),
            },
        ],
    })
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct ScreenUniform {
    size: [f32; 2],
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct RoundedRectInstance {
    bounds: [f32; 4],
    color: [f32; 4],
    round: [f32; 4],
    line_width: f32,
    paint_kind: u32,
    clip_range: [u32; 2],
}

impl RoundedRectInstance {
    fn from_draw(draw: &RoundedRectDraw, clip_masks: ClipMaskRange) -> Option<Self> {
        match *draw {
            RoundedRectDraw::Fill { rect, color, round } if !rect.is_empty() => Some(Self::new(
                rect,
                color,
                round,
                0.0,
                RectPaintKind::Fill,
                clip_masks,
            )),
            RoundedRectDraw::Stroke {
                stroke,
                color,
                round,
            } if !stroke.is_empty() => Some(Self::new(
                stroke.rect(),
                color,
                round,
                stroke.line_width,
                RectPaintKind::Stroke,
                clip_masks,
            )),
            RoundedRectDraw::Fill { .. } | RoundedRectDraw::Stroke { .. } => None,
        }
    }

    fn new(
        rect: Rect,
        color: wgpu::Color,
        round: Round,
        line_width: f32,
        paint_kind: RectPaintKind,
        clip_masks: ClipMaskRange,
    ) -> Self {
        let round = round.fit(rect.width, rect.height);
        Self {
            bounds: [rect.x, rect.y, rect.width, rect.height],
            color: [
                color.r as f32,
                color.g as f32,
                color.b as f32,
                color.a as f32,
            ],
            round: [round.lt, round.rt, round.rb, round.lb],
            line_width,
            paint_kind: paint_kind as u32,
            clip_range: clip_masks.as_array(),
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Float32x4,
            3 => Float32,
            4 => Uint32,
            5 => Uint32x2
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RectPaintKind {
    Fill = 0,
    Stroke = 1,
}

#[cfg(test)]
mod tests {
    use crate::{
        canvas::DrawList,
        clip::ScissorRect,
        shapes::rect::{DrawRectExt, Rect},
    };

    use super::*;

    #[test]
    fn gpu_types_have_wgsl_compatible_layouts() {
        assert_eq!(std::mem::size_of::<ScreenUniform>(), 16);
        assert_eq!(std::mem::size_of::<RoundedRectInstance>(), 64);
        assert_eq!(std::mem::size_of::<ClipMask>(), 32);
    }

    #[test]
    fn rectangle_instance_carries_fitted_corner_radii_and_clip_range() {
        let draw = RoundedRectDraw::Fill {
            rect: Rect::new(0.0, 0.0, 40.0, 20.0),
            color: wgpu::Color::WHITE,
            round: Round {
                lt: 30.0,
                rt: 30.0,
                rb: 0.0,
                lb: 0.0,
            },
        };
        let instance = RoundedRectInstance::from_draw(&draw, ClipMaskRange::new(4, 2))
            .expect("non-empty rectangle should produce an instance");

        assert_eq!(instance.round, [20.0, 20.0, 0.0, 0.0]);
        assert_eq!(instance.clip_range, [4, 2]);
    }

    #[test]
    fn typed_fills_and_strokes_preserve_batch_and_paint_order() {
        let payloads = [
            RoundedRectDraw::Fill {
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                color: wgpu::Color::RED,
                round: Round::default(),
            },
            RoundedRectDraw::Stroke {
                stroke: Stroke::from_rect(Rect::new(2.0, 2.0, 16.0, 16.0), 2.0),
                color: wgpu::Color::BLUE,
                round: Round::default(),
            },
            RoundedRectDraw::Fill {
                rect: Rect::new(6.0, 6.0, 8.0, 8.0),
                color: wgpu::Color::GREEN,
                round: Round::default(),
            },
        ];
        let draws = payloads
            .iter()
            .enumerate()
            .map(|(index, payload)| ResolvedRasterDraw {
                payload,
                clip_masks: ClipMaskRange::new(index as u32, 1),
            })
            .collect::<Vec<_>>();
        let batches = [RasterBatch {
            scissor: ScissorRect::new(0, 0, 20, 20),
            draws: 0..3,
        }];
        let mut instances = Vec::new();
        let mut renderer_batches = Vec::new();

        collect_instances(&draws, &batches, &mut instances, &mut renderer_batches);

        assert_eq!(
            instances
                .iter()
                .map(|instance| instance.paint_kind)
                .collect::<Vec<_>>(),
            vec![
                RectPaintKind::Fill as u32,
                RectPaintKind::Stroke as u32,
                RectPaintKind::Fill as u32,
            ]
        );
        assert_eq!(
            instances
                .iter()
                .map(|instance| instance.clip_range)
                .collect::<Vec<_>>(),
            vec![[0, 1], [1, 1], [2, 1]]
        );
        assert_eq!(renderer_batches, vec![0..3]);
    }

    #[test]
    fn culled_shapes_keep_empty_renderer_batch_slots() {
        let payloads = [
            RoundedRectDraw::Fill {
                rect: Rect::new(0.0, 0.0, 0.0, 20.0),
                color: wgpu::Color::RED,
                round: Round::default(),
            },
            RoundedRectDraw::Stroke {
                stroke: Stroke::new(2.0, 2.0, 16.0, 16.0, 2.0),
                color: wgpu::Color::BLUE,
                round: Round::default(),
            },
        ];
        let draws = payloads
            .iter()
            .map(|payload| ResolvedRasterDraw {
                payload,
                clip_masks: ClipMaskRange::new(0, 0),
            })
            .collect::<Vec<_>>();
        let batches = [
            RasterBatch {
                scissor: ScissorRect::new(0, 0, 20, 20),
                draws: 0..1,
            },
            RasterBatch {
                scissor: ScissorRect::new(0, 0, 20, 20),
                draws: 1..2,
            },
        ];
        let mut instances = Vec::new();
        let mut renderer_batches = Vec::new();

        collect_instances(&draws, &batches, &mut instances, &mut renderer_batches);

        assert_eq!(instances.len(), 1);
        assert_eq!(renderer_batches, vec![0..0, 0..1]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_pixels_use_each_command_corner_radius() {
        let Some(mut surface) = crate::offscreen::OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping offscreen rounded rectangle test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws.draw_rounded_rect(
            Rect::new(2.0, 2.0, 12.0, 12.0),
            wgpu::Color::RED,
            Round {
                lt: 4.0,
                rt: 0.0,
                rb: 0.0,
                lb: 0.0,
            },
        );

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 2, 2), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 13, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 2, 13), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 8), [255, 0, 0, 255]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_pixels_use_rounded_clip_commands() {
        let Some(mut surface) = crate::offscreen::OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping offscreen rounded clip test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws.with_rounded_clip(
            Rect::new(2.0, 2.0, 12.0, 12.0),
            Round {
                lt: 4.0,
                rt: 4.0,
                rb: 4.0,
                lb: 4.0,
            },
            |draws| {
                draws.draw_rect(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED);
            },
        );

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 2, 2), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 8), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 1, 8), [0, 0, 255, 255]);
    }
}
