use bytemuck::{Pod, Zeroable};
use vello::wgpu;
use wgpu::util::DeviceExt;

pub const GLASS_COUNT: u32 = 1_00001;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    resolution_time: [f32; 4],
    light_and_count: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlassInstance {
    rect: [f32; 4],
    optics: [f32; 4],
    tint: [f32; 4],
    motion: [f32; 4],
}

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x4,
    1 => Float32x4,
    2 => Float32x4,
    3 => Float32x4
];

impl GlassInstance {
    fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        }
    }
}

pub struct LiquidGlassRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_count: u32,
}

impl LiquidGlassRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        background: &wgpu::TextureView,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid glass WGSL"),
            source: wgpu::ShaderSource::Wgsl(include_str!("liquid_glass.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("liquid glass frame uniform"),
            contents: bytemuck::bytes_of(&FrameUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let instances = make_instances(GLASS_COUNT);
        // wgpu buffers cannot be zero-sized. Keep one inert element allocated so
        // GLASS_COUNT = 0 is valid; the draw call still uses an empty range.
        let empty_instance = [GlassInstance::zeroed()];
        let uploaded_instances = if instances.is_empty() {
            &empty_instance[..]
        } else {
            &instances[..]
        };
        let instance_label = format!("{} liquid glass instances", instances.len());
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&instance_label),
            contents: bytemuck::cast_slice(uploaded_instances),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("liquid glass backdrop sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquid glass bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquid glass pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("liquid glass pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[GlassInstance::buffer_layout()],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            background,
            &sampler,
        );

        Self {
            pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            uniform_buffer,
            instance_buffer,
            instance_count: instances.len() as u32,
        }
    }

    pub fn set_background(&mut self, device: &wgpu::Device, background: &wgpu::TextureView) {
        self.bind_group = create_bind_group(
            device,
            &self.bind_group_layout,
            &self.uniform_buffer,
            background,
            &self.sampler,
        );
    }

    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        elapsed_seconds: f32,
    ) {
        let light_angle = elapsed_seconds * 0.35 - 0.8;
        let frame = FrameUniform {
            resolution_time: [width as f32, height as f32, elapsed_seconds, 0.0],
            light_and_count: [
                light_angle.cos(),
                light_angle.sin(),
                self.instance_count as f32,
                0.0,
            ],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&frame));

        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("instanced liquid glass pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..6, 0..self.instance_count);
    }

    pub fn instance_count(&self) -> u32 {
        self.instance_count
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    background: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("liquid glass bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(background),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn make_instances(count: u32) -> Vec<GlassInstance> {
    const CANVAS_ASPECT_RATIO: f64 = 1_000.0 / 700.0;
    const TINTS: [[f32; 3]; 6] = [
        [0.72, 0.90, 1.00],
        [0.92, 0.78, 1.00],
        [0.72, 1.00, 0.88],
        [1.00, 0.82, 0.70],
        [0.82, 0.88, 1.00],
        [1.00, 0.92, 0.72],
    ];

    if count == 0 {
        return Vec::new();
    }

    // Derive a grid with roughly the same aspect ratio as the test canvas.
    // Iterating over the requested count (rather than every grid cell) also
    // handles a partially-filled final row without special cases.
    let columns = ((count as f64 * CANVAS_ASPECT_RATIO).sqrt().ceil() as u32).max(1);
    let rows = count.div_ceil(columns);
    let step_x = 940.0 / columns as f32;
    let step_y = 590.0 / rows as f32;
    let base_width = (step_x * 3.05).clamp(100.0, 300.0);
    let base_height = (step_y * 2.15).clamp(100.0, 300.0);

    let mut instances = Vec::with_capacity(count as usize);
    for index in 0..count {
        let row = index / columns;
        let column = index % columns;
        let phase = index as f32 * 0.618_034;
        let tint = TINTS[index as usize % TINTS.len()];
        let x = 30.0 + (column as f32 + 0.5) * step_x + (phase * 1.7).sin() * 5.0;
        let y = 72.0 + (row as f32 + 0.5) * step_y + (phase * 1.3).cos() * 5.0;
        let width = base_width + (index % 7) as f32 * 1.25;
        let height = base_height + (index % 5) as f32;

        instances.push(GlassInstance {
            rect: [x, y, width, height],
            optics: [
                13.0 + (index % 4) as f32 * 2.0,
                10.0 + (index % 6) as f32,
                1.18 + (index % 5) as f32 * 0.025,
                0.35 + (index % 4) as f32 * 0.10,
            ],
            tint: [tint[0], tint[1], tint[2], 0.08 + (index % 3) as f32 * 0.025],
            motion: [
                phase,
                1.5 + (index % 4) as f32 * 0.45,
                (index % 11) as f32 * 0.17,
                0.62 + (index % 5) as f32 * 0.035,
            ],
        });
    }
    instances
}

#[cfg(test)]
mod tests {
    use super::make_instances;

    #[test]
    fn instance_generator_honors_any_requested_count() {
        for count in [0, 1, 2, 999, 1_000, 1_001, 4_096, 10_000] {
            assert_eq!(make_instances(count).len(), count as usize);
        }
    }
}
