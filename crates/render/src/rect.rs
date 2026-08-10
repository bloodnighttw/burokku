//! Retained rectangle commands and their private wgpu renderer.

use bytemuck::{Pod, Zeroable};

use crate::canvas::{DrawCommand, DrawList};

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
}

impl DrawRectExt for DrawList {
    fn draw_rect(&mut self, rect: Rect, color: wgpu::Color) -> &mut Self {
        self.draw(DrawCommand::rect(rect, color))
    }
}

pub(crate) struct RectRenderer {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<RectInstance>,
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
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("render rectangle screen bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
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
            screen_buffer,
            screen_bind_group,
            instance_buffer,
            instance_capacity,
            instances: Vec::new(),
        }
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: &[DrawCommand],
        canvas_size: [f32; 2],
    ) {
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: canvas_size,
                _padding: [0.0; 2],
            }),
        );

        self.instances.clear();
        self.instances
            .extend(commands.iter().filter_map(RectInstance::from_command));
        if self.instances.is_empty() {
            return;
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

    pub(crate) fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if self.instances.is_empty() {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..6, 0..self.instances.len() as u32);
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
}

impl RectInstance {
    fn from_command(command: &DrawCommand) -> Option<Self> {
        let DrawCommand::Rect { rect, color } = command;
        (!rect.is_empty()).then_some(Self {
            bounds: [rect.x, rect.y, rect.width, rect.height],
            color: [
                color.r as f32,
                color.g as f32,
                color.b as f32,
                color.a as f32,
            ],
        })
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

#[cfg(test)]
mod tests {
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
            }]
        );
    }

    #[test]
    fn gpu_types_have_wgsl_compatible_layouts() {
        assert_eq!(std::mem::size_of::<ScreenUniform>(), 16);
        assert_eq!(std::mem::size_of::<RectInstance>(), 32);
    }
}
