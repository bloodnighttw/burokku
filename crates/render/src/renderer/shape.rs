use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{BoxStyle, Canvas, Clip, Color, DrawCommand, Rect};

use super::SurfaceSize;

pub(super) struct ShapeRenderer {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    screen_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    clip_buffer: wgpu::Buffer,
    clip_capacity: u64,
    instances: Vec<ShapeInstance>,
    clips: Vec<GpuClip>,
}

impl ShapeRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let screen_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("render screen uniform"),
            contents: bytemuck::bytes_of(&ScreenUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render screen bind group layout"),
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
        let clip_capacity = std::mem::size_of::<GpuClip>() as u64;
        let clip_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render shape clips"),
            size: clip_capacity,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_bind_group =
            create_bind_group(device, &bind_group_layout, &screen_buffer, &clip_buffer);
        let pipeline = create_pipeline(device, &bind_group_layout, target_format);
        let instance_capacity = std::mem::size_of::<ShapeInstance>() as u64;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render shape instances"),
            size: instance_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            screen_buffer,
            bind_group_layout,
            screen_bind_group,
            instance_buffer,
            instance_capacity,
            clip_buffer,
            clip_capacity,
            instances: Vec::new(),
            clips: Vec::new(),
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas: &Canvas,
        size: SurfaceSize,
    ) {
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::bytes_of(&ScreenUniform {
                size: [size.width as f32, size.height as f32],
                _padding: [0.0; 2],
            }),
        );
        self.instances.clear();
        self.clips.clear();
        for command in canvas.commands() {
            let DrawCommand::Box { rect, style, clips } = command else {
                continue;
            };
            if rect.width <= 0.0
                || rect.height <= 0.0
                || clips
                    .iter()
                    .any(|clip| clip.rect.width <= 0.0 || clip.rect.height <= 0.0)
            {
                continue;
            }

            let clip_start = self.clips.len() as u32;
            self.clips.extend(clips.iter().copied().map(GpuClip::new));
            self.instances.push(ShapeInstance::new(
                *rect,
                *style,
                clip_start,
                clips.len() as u32,
            ));
        }
        if self.instances.is_empty() {
            return;
        }

        let bytes = bytemuck::cast_slice(&self.instances);
        let required_capacity = bytes.len() as u64;
        if required_capacity > self.instance_capacity {
            self.instance_capacity = required_capacity.next_power_of_two();
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("render shape instances"),
                size: self.instance_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.instance_buffer, 0, bytes);

        if !self.clips.is_empty() {
            let clip_bytes = bytemuck::cast_slice(&self.clips);
            let required_clip_capacity = clip_bytes.len() as u64;
            if required_clip_capacity > self.clip_capacity {
                self.clip_capacity = required_clip_capacity.next_power_of_two();
                self.clip_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("render shape clips"),
                    size: self.clip_capacity,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.screen_bind_group = create_bind_group(
                    device,
                    &self.bind_group_layout,
                    &self.screen_buffer,
                    &self.clip_buffer,
                );
            }
            queue.write_buffer(&self.clip_buffer, 0, clip_bytes);
        }
    }

    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if self.instances.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..6, 0..self.instances.len() as u32);
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    screen_buffer: &wgpu::Buffer,
    clip_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("render screen and clips bind group"),
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

fn create_pipeline(
    device: &wgpu::Device,
    screen_layout: &wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("render built-in shape shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/shape.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("render shape pipeline layout"),
        bind_group_layouts: &[Some(screen_layout)],
        immediate_size: 0,
    });
    let targets = [Some(wgpu::ColorTargetState {
        format: target_format,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("render shape pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[ShapeInstance::layout()],
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
struct ShapeInstance {
    center: [f32; 2],
    half_size: [f32; 2],
    radii: [f32; 4],
    background: [f32; 4],
    border_color: [f32; 4],
    outline_color: [f32; 4],
    border_width: f32,
    outline_width: f32,
    outline_offset: f32,
    _padding: f32,
    clip_range: [u32; 2],
}

impl ShapeInstance {
    fn new(rect: Rect, style: BoxStyle, clip_start: u32, clip_count: u32) -> Self {
        let border = style.border.filter(|border| border.width > 0.0);
        let outline = style.outline.filter(|outline| outline.width > 0.0);
        Self {
            center: [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5],
            half_size: [rect.width * 0.5, rect.height * 0.5],
            radii: style.corner_radius.normalized(rect),
            background: style.background.components(),
            border_color: border
                .map_or(Color::TRANSPARENT, |border| border.color)
                .components(),
            outline_color: outline
                .map_or(Color::TRANSPARENT, |outline| outline.color)
                .components(),
            border_width: border.map_or(0.0, |border| {
                border.width.clamp(0.0, rect.width.min(rect.height) * 0.5)
            }),
            outline_width: outline.map_or(0.0, |outline| outline.width.max(0.0)),
            outline_offset: outline.map_or(0.0, |outline| outline.offset.max(0.0)),
            _padding: 0.0,
            clip_range: [clip_start, clip_count],
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 11] = wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x2,
            2 => Float32x4,
            3 => Float32x4,
            4 => Float32x4,
            5 => Float32x4,
            6 => Float32,
            7 => Float32,
            8 => Float32,
            9 => Float32,
            10 => Uint32x2
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct GpuClip {
    center: [f32; 2],
    half_size: [f32; 2],
    radii: [f32; 4],
}

impl GpuClip {
    fn new(clip: Clip) -> Self {
        Self {
            center: [
                clip.rect.x + clip.rect.width * 0.5,
                clip.rect.y + clip.rect.height * 0.5,
            ],
            half_size: [clip.rect.width * 0.5, clip.rect.height * 0.5],
            radii: clip.corner_radius.normalized(clip.rect),
        }
    }
}
