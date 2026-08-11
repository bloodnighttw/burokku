//! Retained rectangle commands and their private wgpu renderer.

use bytemuck::{Pod, Zeroable};
use std::ops::Range;

use crate::{
    canvas::{DrawCommand, DrawList},
    clip::{ClipMask, ClipStack, ScissorRect},
    shapes::round::Round,
};

/// A rectangle in logical canvas pixels.
///
/// Coordinates start at the canvas's top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// Convenience rectangle recording for real frames and mock [`DrawList`]s.
pub trait DrawRectExt {
    fn draw_rect(&mut self, rect: Rect, color: wgpu::Color) -> &mut Self;
    fn draw_rounded_rect(&mut self, rect: Rect, color: wgpu::Color, round: Round) -> &mut Self;
}

impl DrawRectExt for DrawList {
    fn draw_rect(&mut self, rect: Rect, color: wgpu::Color) -> &mut Self {
        self.draw_rounded_rect(rect, color, Round::default())
    }

    fn draw_rounded_rect(&mut self, rect: Rect, color: wgpu::Color, round: Round) -> &mut Self {
        self.draw(DrawCommand::rect(rect, color, round))
    }
}

pub(crate) struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    clip_buffer: wgpu::Buffer,
    clip_capacity: u64,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<RectInstance>,
    clip_masks: Vec<ClipMask>,
    batches: Vec<RectBatch>,
}

impl RectRenderer {
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render rectangle screen uniform"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render rectangle bind group layout"),
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
            label: Some("render rectangle shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("wgsl/rect.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render rectangle pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let targets = [Some(wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render rectangle pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[RectInstance::layout()],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let instance_capacity = std::mem::size_of::<RectInstance>() as u64;
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
            clip_masks: Vec::new(),
            batches: Vec::new(),
        }
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: &[DrawCommand],
        canvas_size: [u32; 2],
    ) {
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: [canvas_size[0] as f32, canvas_size[1] as f32],
                _padding: [0.0; 2],
            }),
        );

        self.instances.clear();
        self.clip_masks.clear();
        self.batches.clear();
        collect_instances(
            commands,
            canvas_size,
            &mut self.instances,
            &mut self.clip_masks,
            &mut self.batches,
        );
        if self.instances.is_empty() {
            return;
        }

        let required_clips = std::mem::size_of_val(self.clip_masks.as_slice()) as u64;
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
        if !self.clip_masks.is_empty() {
            queue.write_buffer(&self.clip_buffer, 0, bytemuck::cast_slice(&self.clip_masks));
        }

        let required = std::mem::size_of_val(self.instances.as_slice()) as u64;
        if required > self.instance_capacity {
            self.instance_capacity = required.next_power_of_two();
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
        }
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }

    pub(crate) fn batch_count(&self) -> usize {
        self.batches.len()
    }

    pub(crate) fn batch_order(&self, index: usize) -> usize {
        self.batches[index].order
    }

    pub(crate) fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, index: usize) {
        let batch = &self.batches[index];
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.set_scissor_rect(
            batch.scissor.x,
            batch.scissor.y,
            batch.scissor.width,
            batch.scissor.height,
        );
        pass.draw(0..6, batch.instances.clone());
    }
}

fn collect_instances(
    commands: &[DrawCommand],
    canvas_size: [u32; 2],
    instances: &mut Vec<RectInstance>,
    clip_masks: &mut Vec<ClipMask>,
    batches: &mut Vec<RectBatch>,
) {
    let mut clips = ClipStack::new(canvas_size);
    let mut rounded_clips = Vec::new();

    for (order, command) in commands.iter().enumerate() {
        match command {
            DrawCommand::PushClip { rect, round } => {
                clips.push(*rect);
                let mask = ClipMask::new(*rect, *round);
                rounded_clips.push(mask.is_rounded().then_some(mask));
            }
            DrawCommand::PopClip => {
                clips.pop();
                rounded_clips.pop();
            }
            DrawCommand::Rect { rect, color, round } => {
                let active_clip = clips.active();
                if rect.is_empty() || active_clip.is_empty() {
                    continue;
                }

                let instance = RectInstance::new(*rect, *color, *round);
                push_instance(
                    instance,
                    active_clip,
                    &rounded_clips,
                    order,
                    instances,
                    clip_masks,
                    batches,
                );
            }
            _ => {}
        }
    }
}

fn push_instance(
    mut instance: RectInstance,
    scissor: ScissorRect,
    rounded_clips: &[Option<ClipMask>],
    order: usize,
    instances: &mut Vec<RectInstance>,
    clip_masks: &mut Vec<ClipMask>,
    batches: &mut Vec<RectBatch>,
) {
    let clip_start = clip_masks.len() as u32;
    clip_masks.extend(rounded_clips.iter().flatten().copied());
    instance.clip_range = [clip_start, clip_masks.len() as u32 - clip_start];

    let instance_index = instances.len() as u32;
    instances.push(instance);

    match batches.last_mut() {
        Some(batch) if batch.scissor == scissor && batch.last_order + 1 == order => {
            batch.instances.end = instance_index + 1;
            batch.last_order = order;
        }
        _ => batches.push(RectBatch {
            scissor,
            instances: instance_index..instance_index + 1,
            order,
            last_order: order,
        }),
    }
}

fn create_instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render rectangle instance buffer"),
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
        label: Some("render rectangle bind group"),
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
struct RectInstance {
    bounds: [f32; 4],
    color: [f32; 4],
    round: [f32; 4],
    clip_range: [u32; 2],
}

impl RectInstance {
    fn new(rect: Rect, color: wgpu::Color, round: Round) -> Self {
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
            clip_range: [0; 2],
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Float32x4,
            3 => Uint32x2
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RectBatch {
    scissor: ScissorRect,
    instances: Range<u32>,
    order: usize,
    last_order: usize,
}

#[cfg(test)]
mod tests {
    use crate::shapes::round;

    use super::*;

    #[test]
    fn non_positive_rectangles_are_empty() {
        assert!(Rect::new(0.0, 0.0, 0.0, 10.0).is_empty());
        assert!(Rect::new(0.0, 0.0, 10.0, -1.0).is_empty());
        assert!(!Rect::new(0.0, 0.0, 10.0, 20.0).is_empty());
    }

    #[test]
    fn extension_records_a_rectangle_without_a_gpu() {
        let mut draws = DrawList::new();
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);

        draws.draw_rect(rect, wgpu::Color::GREEN);

        assert_eq!(
            draws.commands(),
            &[DrawCommand::Rect {
                rect,
                color: wgpu::Color::GREEN,
                round: crate::shapes::round::Round::default(),
            }]
        );
    }

    #[test]
    fn extension_records_a_rounded_rectangle_without_a_gpu() {
        let mut draws = DrawList::new();
        let rect = Rect::new(1.0, 2.0, 30.0, 40.0);
        let round = Round {
            lt: 1.0,
            rt: 2.0,
            rb: 3.0,
            lb: 4.0,
        };

        draws.draw_rounded_rect(rect, wgpu::Color::GREEN, round);

        assert_eq!(
            draws.commands(),
            &[DrawCommand::Rect {
                rect,
                color: wgpu::Color::GREEN,
                round,
            }]
        );
    }

    #[test]
    fn gpu_types_have_wgsl_compatible_layouts() {
        assert_eq!(std::mem::size_of::<ScreenUniform>(), 16);
        assert_eq!(std::mem::size_of::<RectInstance>(), 56);
        assert_eq!(std::mem::size_of::<ClipMask>(), 32);
    }

    #[test]
    fn rectangle_instance_carries_fitted_corner_radii() {
        let instance = RectInstance::new(
            Rect::new(0.0, 0.0, 40.0, 20.0),
            wgpu::Color::WHITE,
            Round {
                lt: 30.0,
                rt: 30.0,
                rb: 0.0,
                lb: 0.0,
            },
        );

        assert_eq!(instance.round, [20.0, 20.0, 0.0, 0.0]);
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

    #[test]
    fn nested_clips_create_ordered_scissor_batches() {
        let commands = [
            DrawCommand::rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                wgpu::Color::RED,
                round::Round::default(),
            ),
            DrawCommand::push_clip(Rect::new(10.0, 10.0, 50.0, 50.0), round::Round::default()),
            DrawCommand::rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                wgpu::Color::GREEN,
                round::Round::default(),
            ),
            DrawCommand::push_clip(Rect::new(40.0, 0.0, 50.0, 30.0), round::Round::default()),
            DrawCommand::rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                wgpu::Color::BLUE,
                round::Round::default(),
            ),
            DrawCommand::pop_clip(),
            DrawCommand::rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                wgpu::Color::WHITE,
                round::Round::default(),
            ),
            DrawCommand::pop_clip(),
            DrawCommand::rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                wgpu::Color::BLACK,
                round::Round::default(),
            ),
        ];
        let mut instances = Vec::new();
        let mut clip_masks = Vec::new();
        let mut batches = Vec::new();

        collect_instances(
            &commands,
            [100, 100],
            &mut instances,
            &mut clip_masks,
            &mut batches,
        );

        assert_eq!(instances.len(), 5);
        assert_eq!(
            batches,
            vec![
                RectBatch {
                    scissor: ScissorRect::new(0, 0, 100, 100),
                    instances: 0..1,
                    order: 0,
                    last_order: 0,
                },
                RectBatch {
                    scissor: ScissorRect::new(10, 10, 50, 50),
                    instances: 1..2,
                    order: 2,
                    last_order: 2,
                },
                RectBatch {
                    scissor: ScissorRect::new(40, 10, 20, 20),
                    instances: 2..3,
                    order: 4,
                    last_order: 4,
                },
                RectBatch {
                    scissor: ScissorRect::new(10, 10, 50, 50),
                    instances: 3..4,
                    order: 6,
                    last_order: 6,
                },
                RectBatch {
                    scissor: ScissorRect::new(0, 0, 100, 100),
                    instances: 4..5,
                    order: 8,
                    last_order: 8,
                },
            ]
        );
    }

    #[test]
    fn empty_nested_clip_discards_its_rectangles() {
        let commands = [
            DrawCommand::push_clip(Rect::new(200.0, 200.0, 10.0, 10.0), round::Round::default()),
            DrawCommand::rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                wgpu::Color::RED,
                round::Round::default(),
            ),
            DrawCommand::pop_clip(),
        ];
        let mut instances = Vec::new();
        let mut clip_masks = Vec::new();
        let mut batches = Vec::new();

        collect_instances(
            &commands,
            [100, 100],
            &mut instances,
            &mut clip_masks,
            &mut batches,
        );

        assert!(instances.is_empty());
        assert!(batches.is_empty());
    }
}
