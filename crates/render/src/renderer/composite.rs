use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{Clip, Transform};

use super::SurfaceSize;

pub(super) struct CompositeRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

pub(super) struct CompositeItem {
    _texture: wgpu::Texture,
    _uniform: wgpu::Buffer,
    _clip_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    clips: Vec<Clip>,
}

pub(super) struct CompositeEffect {
    pub origin: [f32; 2],
    pub transform: Transform,
    pub opacity: f32,
    pub clips: Vec<Clip>,
}

impl CompositeRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render group composite bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("render group composite sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render group composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/composite.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render group composite pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render group composite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
        }
    }

    pub fn item(
        &self,
        device: &wgpu::Device,
        texture: wgpu::Texture,
        size: SurfaceSize,
        effect: CompositeEffect,
    ) -> Option<CompositeItem> {
        let CompositeEffect {
            origin,
            transform,
            opacity,
            clips,
        } = effect;
        let maximum_clip_bytes =
            (device.limits().max_storage_buffer_binding_size as usize).min(1024 * 1024);
        if clips
            .len()
            .max(1)
            .checked_mul(std::mem::size_of::<CompositeClip>())
            .is_none_or(|bytes| bytes > maximum_clip_bytes)
        {
            return None;
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let [a, b, c, d, tx, ty] = transform.matrix;
        let gpu_clips = if clips.is_empty() {
            vec![CompositeClip::default()]
        } else {
            clips.iter().copied().map(CompositeClip::new).collect()
        };
        let clip_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("render group composite clips"),
            contents: bytemuck::cast_slice(&gpu_clips),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("render group composite uniform"),
            contents: bytemuck::bytes_of(&CompositeUniform {
                screen_origin: [size.width as f32, size.height as f32, origin[0], origin[1]],
                transform_x: [a, c, tx, opacity.clamp(0.0, 1.0)],
                transform_y: [b, d, ty, clips.len() as f32],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("render group composite bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: clip_buffer.as_entire_binding(),
                },
            ],
        });
        Some(CompositeItem {
            _texture: texture,
            _uniform: uniform,
            _clip_buffer: clip_buffer,
            bind_group,
            clips,
        })
    }

    pub fn draw<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        items: &'pass [CompositeItem],
        size: SurfaceSize,
    ) {
        if items.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        for item in items {
            let clip = item.clips.iter().fold(
                crate::Rect::new(0.0, 0.0, size.width as f32, size.height as f32),
                |bounds, clip| bounds.intersection(clip.bounds()),
            );
            if clip.width <= 0.0 || clip.height <= 0.0 {
                continue;
            }
            let x = clip.x.floor().max(0.0) as u32;
            let y = clip.y.floor().max(0.0) as u32;
            let right = (clip.x + clip.width).ceil().min(size.width as f32) as u32;
            let bottom = (clip.y + clip.height).ceil().min(size.height as f32) as u32;
            if right <= x || bottom <= y {
                continue;
            }
            pass.set_scissor_rect(x, y, right - x, bottom - y);
            pass.set_bind_group(0, &item.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        pass.set_scissor_rect(0, 0, size.width, size.height);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompositeUniform {
    screen_origin: [f32; 4],
    transform_x: [f32; 4],
    transform_y: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
struct CompositeClip {
    center: [f32; 2],
    half_size: [f32; 2],
    radii: [f32; 4],
    inverse_x: [f32; 3],
    _padding_x: f32,
    inverse_y: [f32; 3],
    _padding_y: f32,
}

impl CompositeClip {
    fn new(clip: Clip) -> Self {
        let [a, b, c, d, tx, ty] = clip.transform;
        let determinant = a * d - b * c;
        let (inverse_x, inverse_y) = if determinant.abs() > f32::EPSILON {
            let inverse = [
                d / determinant,
                -b / determinant,
                -c / determinant,
                a / determinant,
            ];
            (
                [inverse[0], inverse[2], -inverse[0] * tx - inverse[2] * ty],
                [inverse[1], inverse[3], -inverse[1] * tx - inverse[3] * ty],
            )
        } else {
            ([0.0; 3], [0.0; 3])
        };
        Self {
            center: [
                clip.rect.x + clip.rect.width * 0.5,
                clip.rect.y + clip.rect.height * 0.5,
            ],
            half_size: [clip.rect.width * 0.5, clip.rect.height * 0.5],
            radii: clip.corner_radius.normalized(clip.rect),
            inverse_x,
            _padding_x: 0.0,
            inverse_y,
            _padding_y: 0.0,
        }
    }
}
