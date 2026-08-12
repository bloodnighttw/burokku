use std::{env, fs::File, io, mem, path::PathBuf};

use bytemuck::{Pod, Zeroable};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use render::{
    backdrop::{
        BackdropCreateContext, BackdropPrepareContext, BackdropRenderer, BackdropRendererFactory,
        BackdropRendererHandle, ResolvedBackdropDraw,
    },
    canvas::DrawList,
    offscreen::OffscreenSurface,
    shapes::{
        rect::{DrawRectExt, Rect},
        round::Round,
        stroke::{DrawStrokeExt, Stroke},
    },
    wgpu,
};

const SIZE: [u32; 2] = [800, 500];

/*
The shader below adapts the rendering approach from
https://github.com/whynotmake-it/flutter_liquid_glass:
rounded-rectangle SDF geometry, a curved surface normal, refracted backdrop
sampling, chromatic dispersion, glass tint, saturation, and rim lighting.

Copyright 2025 Tim Lehmann for whynotmake.it

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

The renderer runs the same three logical stages as upstream: separable Gaussian
backdrop blur, displacement geometry, and final glass composition. Geometry and
composition are combined in the final WGSL pass because this example has one
static shape and does not need upstream's reusable geometry-texture cache.
*/
const LIQUID_GLASS_WGSL: &str = r#"
const LIQUID_LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

fn liquid_rounded_rect_sdf(
    position: vec2<f32>,
    size: vec2<f32>,
    radius: f32,
) -> f32 {
    let half_size = size * 0.5;
    let centered = position - half_size;
    let corner = abs(centered) - half_size + vec2<f32>(radius);
    return min(max(corner.x, corner.y), 0.0)
        + length(max(corner, vec2<f32>(0.0))) - radius;
}

fn backdrop_main(input: WgslInput, params: array<vec4<f32>, 4>) -> vec4<f32> {
    // tint: rgba
    let tint = params[0];
    // optical: refractive index, chromatic aberration, thickness px, unused
    let optical = params[1];
    // light: direction xy, intensity, ambient strength
    let light = params[2];
    // geometry: canvas width, canvas height, saturation, corner radius px
    let geometry = params[3];

    let size = input.bounds.zw;
    let radius = clamp(geometry.w, 0.0, min(size.x, size.y) * 0.5);
    let thickness = max(optical.z, 0.001);
    let distance = liquid_rounded_rect_sdf(input.local_position, size, radius);
    let foreground_alpha = select(
        0.0,
        1.0 - smoothstep(-2.0, 0.0, distance),
        distance < 0.0,
    );

    let edge_depth = thickness + distance;
    let curved_height = sqrt(max(0.0, thickness * thickness - edge_depth * edge_depth));
    let height = select(curved_height, thickness, distance < -thickness);
    let gradient = vec2<f32>(dpdx(distance), dpdy(distance));
    let normal_xy = max(thickness + distance, 0.0) / thickness;
    let normal_z = sqrt(max(0.0, 1.0 - normal_xy * normal_xy));
    let normal = normalize(vec3<f32>(gradient * normal_xy, normal_z));

    let incident = vec3<f32>(0.0, 0.0, -1.0);
    let refracted_ray = refract(incident, normal, 1.0 / max(optical.x, 1.001));
    let travel = (height + thickness * 8.0) / max(abs(refracted_ray.z), 0.001);
    let displacement_px = refracted_ray.xy * travel;
    let texel_size = vec2<f32>(1.0) / max(geometry.xy, vec2<f32>(1.0));
    let dispersion = optical.y * 0.5;

    let red_uv = input.screen_uv + displacement_px * (1.0 + dispersion) * texel_size;
    let green_uv = input.screen_uv + displacement_px * texel_size;
    let blue_uv = input.screen_uv + displacement_px * (1.0 - dispersion) * texel_size;
    let red = sample_backdrop(red_uv);
    let green = sample_backdrop(green_uv);
    let blue = sample_backdrop(blue_uv);
    let refracted = vec4<f32>(red.r, green.g, blue.b, green.a);

    // Port of liquid_glass_final_render.frag from the current renderer.
    var glass = tint.rgb * tint.a + refracted.rgb * (1.0 - tint.a);
    let luminance = dot(glass, LIQUID_LUMA);
    glass = clamp(mix(vec3<f32>(luminance), glass, geometry.z), vec3<f32>(0.0), vec3<f32>(1.0));

    let normalized_height = height / thickness;
    let thickness_scale = clamp(40.0 / max(thickness, 1.0), 1.0, 4.0);
    let edge_threshold = mix(0.8, 0.5, 1.0 / thickness_scale);
    let edge_factor = 1.0 - smoothstep(0.0, edge_threshold, normalized_height);
    if edge_factor > 0.01 {
        let displacement_length = length(displacement_px);
        let edge_normal = displacement_px / max(displacement_length, 0.001);
        let light_direction = light.xy / max(length(light.xy), 0.001);
        let main_light = max(dot(edge_normal, light_direction), 0.0);
        let opposite_light = max(dot(edge_normal, -light_direction), 0.0);
        let influence = main_light + opposite_light * 0.8;
        let directional = pow(influence, 1.5) * light.z * 3.0;
        let ambient = light.w * 0.5;
        let brightness = (directional + ambient) * edge_factor * thickness_scale * 0.8;

        let background_luminance = dot(refracted.rgb, LIQUID_LUMA);
        var saturated_background = refracted.rgb / max(background_luminance, 0.001);
        saturated_background = mix(refracted.rgb, saturated_background, 0.8);
        let colorfulness = length(refracted.rgb - vec3<f32>(background_luminance));
        let color_mix = clamp(colorfulness + 0.5, 0.5, 1.0);
        let highlight = mix(vec3<f32>(1.0), saturated_background, color_mix);
        glass = mix(glass, highlight, brightness);
    }

    return vec4<f32>(
        clamp(glass, vec3<f32>(0.0), vec3<f32>(1.0)),
        foreground_alpha,
    );
}
"#;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiquidGlassDraw {
    bounds: Rect,
    round: Round,
    blur_sigma: f32,
    params: [[f32; 4]; 4],
}

struct LiquidGlass {
    handle: BackdropRendererHandle<LiquidGlassDraw>,
}

impl LiquidGlass {
    fn new() -> Self {
        Self {
            handle: BackdropRendererHandle::new("liquid glass", LiquidGlassFactory),
        }
    }

    fn draw_rounded<'draws>(
        &self,
        draws: &'draws mut DrawList,
        bounds: Rect,
        round: Round,
        blur_sigma: f32,
        params: [[f32; 4]; 4],
    ) -> &'draws mut DrawList {
        draws.backdrop_with(
            &self.handle,
            LiquidGlassDraw {
                bounds,
                round,
                blur_sigma: blur_sigma.max(0.0),
                params,
            },
        )
    }
}

#[derive(Clone, Copy)]
struct LiquidGlassFactory;

impl BackdropRendererFactory<LiquidGlassDraw> for LiquidGlassFactory {
    type Renderer = LiquidGlassRenderer;

    fn create(&self, context: BackdropCreateContext<'_>) -> Self::Renderer {
        LiquidGlassRenderer::new(context)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LiquidGlassInstance {
    bounds: [f32; 4],
    round: [f32; 4],
    clip_range: [u32; 2],
    padding: [u32; 2],
    params: [[f32; 4]; 4],
}

struct LiquidGlassRenderer {
    effect_pipeline: wgpu::RenderPipeline,
    effect_layout: wgpu::BindGroupLayout,
    screen_buffer: wgpu::Buffer,
    clip_buffer: wgpu::Buffer,
    clip_capacity: u64,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    effect_bind_group: wgpu::BindGroup,
    instance_indices: Vec<Option<u32>>,
    horizontal_blur_pipeline: wgpu::RenderPipeline,
    vertical_blur_pipeline: wgpu::RenderPipeline,
    blur_settings_layout: wgpu::BindGroupLayout,
    blur_settings_buffer: wgpu::Buffer,
    blur_settings_capacity: u64,
    blur_settings_bind_group: wgpu::BindGroup,
    scene_layout: wgpu::BindGroupLayout,
    blur_sampler: wgpu::Sampler,
    target_format: wgpu::TextureFormat,
    blur_resources: Option<BlurResources>,
}

struct BlurResources {
    size: [u32; 2],
    _textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    source_bind_groups: [wgpu::BindGroup; 2],
}

impl LiquidGlassRenderer {
    fn new(context: BackdropCreateContext<'_>) -> Self {
        const INITIAL_BUFFER_SIZE: u64 = 16;

        let effect_layout = create_effect_layout(context.device);
        let screen_buffer = create_buffer(
            context.device,
            "liquid glass screen uniform",
            16,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let clip_buffer = create_buffer(
            context.device,
            "liquid glass clip masks",
            INITIAL_BUFFER_SIZE,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let instance_capacity = mem::size_of::<LiquidGlassInstance>() as u64;
        let instance_buffer = create_buffer(
            context.device,
            "liquid glass instances",
            instance_capacity,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let effect_bind_group = create_effect_bind_group(
            context.device,
            &effect_layout,
            &screen_buffer,
            &clip_buffer,
            &instance_buffer,
        );
        let effect_source = format!(
            "{}\n{}\n{}",
            LIQUID_GLASS_SHADER_PREFIX, LIQUID_GLASS_WGSL, LIQUID_GLASS_SHADER_SUFFIX
        );
        let effect_shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("liquid glass effect shader"),
                source: wgpu::ShaderSource::Wgsl(effect_source.into()),
            });
        let effect_pipeline = create_effect_pipeline(
            context.device,
            context.target_format,
            context.scene_bind_group_layout,
            &effect_layout,
            &effect_shader,
        );

        let blur_settings_layout = create_blur_settings_layout(context.device);
        let blur_settings_buffer = create_buffer(
            context.device,
            "liquid glass blur settings",
            INITIAL_BUFFER_SIZE,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let blur_settings_bind_group = create_blur_settings_bind_group(
            context.device,
            &blur_settings_layout,
            &blur_settings_buffer,
        );
        let blur_shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("liquid glass Gaussian blur shader"),
                source: wgpu::ShaderSource::Wgsl(GAUSSIAN_BLUR_WGSL.into()),
            });
        let horizontal_blur_pipeline = create_blur_pipeline(
            context.device,
            context.target_format,
            context.scene_bind_group_layout,
            &blur_settings_layout,
            &blur_shader,
            "blur_horizontal",
        );
        let vertical_blur_pipeline = create_blur_pipeline(
            context.device,
            context.target_format,
            context.scene_bind_group_layout,
            &blur_settings_layout,
            &blur_shader,
            "blur_vertical",
        );
        let blur_sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("liquid glass blur sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            effect_pipeline,
            effect_layout,
            screen_buffer,
            clip_buffer,
            clip_capacity: INITIAL_BUFFER_SIZE,
            instance_buffer,
            instance_capacity,
            effect_bind_group,
            instance_indices: Vec::new(),
            horizontal_blur_pipeline,
            vertical_blur_pipeline,
            blur_settings_layout,
            blur_settings_buffer,
            blur_settings_capacity: INITIAL_BUFFER_SIZE,
            blur_settings_bind_group,
            scene_layout: context.scene_bind_group_layout.clone(),
            blur_sampler,
            target_format: context.target_format,
            blur_resources: None,
        }
    }

    fn prepare_effect(
        &mut self,
        context: BackdropPrepareContext<'_>,
        draws: &[ResolvedBackdropDraw<'_, LiquidGlassDraw>],
    ) {
        let mut instances = Vec::with_capacity(draws.len());
        self.instance_indices.clear();
        for draw in draws {
            if draw.payload.bounds.is_empty() {
                self.instance_indices.push(None);
                continue;
            }
            let round = fit_round(
                draw.payload.round,
                draw.payload.bounds.width,
                draw.payload.bounds.height,
            );
            self.instance_indices.push(Some(instances.len() as u32));
            instances.push(LiquidGlassInstance {
                bounds: [
                    draw.payload.bounds.x,
                    draw.payload.bounds.y,
                    draw.payload.bounds.width,
                    draw.payload.bounds.height,
                ],
                round: [round.lt, round.rt, round.rb, round.lb],
                clip_range: draw.clip_masks.as_array(),
                padding: [0; 2],
                params: draw.payload.params,
            });
        }

        let required_clips = mem::size_of_val(context.clip_masks) as u64;
        let required_instances = mem::size_of_val(instances.as_slice()) as u64;
        let mut recreate_bind_group = false;
        if required_clips > self.clip_capacity {
            self.clip_capacity = required_clips.next_power_of_two();
            self.clip_buffer = create_buffer(
                context.device,
                "liquid glass clip masks",
                self.clip_capacity,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            recreate_bind_group = true;
        }
        if required_instances > self.instance_capacity {
            self.instance_capacity = required_instances.next_power_of_two();
            self.instance_buffer = create_buffer(
                context.device,
                "liquid glass instances",
                self.instance_capacity,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            recreate_bind_group = true;
        }
        if recreate_bind_group {
            self.effect_bind_group = create_effect_bind_group(
                context.device,
                &self.effect_layout,
                &self.screen_buffer,
                &self.clip_buffer,
                &self.instance_buffer,
            );
        }

        context.queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::cast_slice(&[
                context.canvas_size[0] as f32,
                context.canvas_size[1] as f32,
                0.0,
                0.0,
            ]),
        );
        if !context.clip_masks.is_empty() {
            context.queue.write_buffer(
                &self.clip_buffer,
                0,
                bytemuck::cast_slice(context.clip_masks),
            );
        }
        if !instances.is_empty() {
            context
                .queue
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }
    }

    fn prepare_blur(
        &mut self,
        context: BackdropPrepareContext<'_>,
        draws: &[ResolvedBackdropDraw<'_, LiquidGlassDraw>],
    ) {
        if self
            .blur_resources
            .as_ref()
            .is_none_or(|resources| resources.size != context.canvas_size)
        {
            self.blur_resources = Some(create_blur_resources(
                context.device,
                &self.scene_layout,
                &self.blur_sampler,
                self.target_format,
                context.canvas_size,
            ));
        }

        let settings = draws
            .iter()
            .map(|draw| [draw.payload.blur_sigma.max(0.001), 0.0, 0.0, 0.0])
            .collect::<Vec<_>>();
        let required = mem::size_of_val(settings.as_slice()) as u64;
        if required > self.blur_settings_capacity {
            self.blur_settings_capacity = required.next_power_of_two();
            self.blur_settings_buffer = create_buffer(
                context.device,
                "liquid glass blur settings",
                self.blur_settings_capacity,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            self.blur_settings_bind_group = create_blur_settings_bind_group(
                context.device,
                &self.blur_settings_layout,
                &self.blur_settings_buffer,
            );
        }
        if !settings.is_empty() {
            context.queue.write_buffer(
                &self.blur_settings_buffer,
                0,
                bytemuck::cast_slice(&settings),
            );
        }
    }
}

impl BackdropRenderer<LiquidGlassDraw> for LiquidGlassRenderer {
    fn prepare(
        &mut self,
        context: BackdropPrepareContext<'_>,
        draws: &[ResolvedBackdropDraw<'_, LiquidGlassDraw>],
    ) {
        self.prepare_effect(context, draws);
        self.prepare_blur(context, draws);
    }

    fn encode_source<'resource>(
        &'resource self,
        encoder: &mut wgpu::CommandEncoder,
        source_bind_group: &'resource wgpu::BindGroup,
        draw_index: usize,
    ) -> &'resource wgpu::BindGroup {
        let resources = self
            .blur_resources
            .as_ref()
            .expect("liquid glass blur resources must be prepared before encoding");
        encode_blur_pass(
            encoder,
            &resources.views[0],
            &self.horizontal_blur_pipeline,
            source_bind_group,
            &self.blur_settings_bind_group,
            draw_index,
            "liquid glass horizontal blur",
        );
        encode_blur_pass(
            encoder,
            &resources.views[1],
            &self.vertical_blur_pipeline,
            &resources.source_bind_groups[0],
            &self.blur_settings_bind_group,
            draw_index,
            "liquid glass vertical blur",
        );
        &resources.source_bind_groups[1]
    }

    fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, draw_index: usize) {
        let Some(instance) = self.instance_indices[draw_index] else {
            return;
        };
        pass.set_pipeline(&self.effect_pipeline);
        pass.set_bind_group(1, &self.effect_bind_group, &[]);
        pass.draw(0..6, instance..instance + 1);
    }
}

fn create_effect_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("liquid glass effect resources layout"),
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
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn create_effect_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    screen_buffer: &wgpu::Buffer,
    clip_buffer: &wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("liquid glass effect resources"),
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
            wgpu::BindGroupEntry {
                binding: 2,
                resource: instance_buffer.as_entire_binding(),
            },
        ],
    })
}

fn create_effect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    scene_layout: &wgpu::BindGroupLayout,
    effect_layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("liquid glass effect pipeline layout"),
        bind_group_layouts: &[Some(scene_layout), Some(effect_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("liquid glass effect pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("liquid_vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("liquid_fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_blur_settings_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("liquid glass blur settings layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn create_blur_settings_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("liquid glass blur settings"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn create_blur_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    scene_layout: &wgpu::BindGroupLayout,
    settings_layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &str,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("liquid glass blur pipeline layout"),
        bind_group_layouts: &[Some(scene_layout), Some(settings_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment_entry),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("blur_vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_blur_resources(
    device: &wgpu::Device,
    scene_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) -> BlurResources {
    let create_texture = |axis: &str| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(axis),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    let textures = [
        create_texture("liquid glass horizontal blur texture"),
        create_texture("liquid glass vertical blur texture"),
    ];
    let views = textures
        .each_ref()
        .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
    let source_bind_groups = views.each_ref().map(|view| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid glass blurred source"),
            layout: scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    });
    BlurResources {
        size,
        _textures: textures,
        views,
        source_bind_groups,
    }
}

fn encode_blur_pass(
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    source: &wgpu::BindGroup,
    settings: &wgpu::BindGroup,
    draw_index: usize,
    label: &str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: destination,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let instance = u32::try_from(draw_index).expect("liquid glass draw index must fit in u32");
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, source, &[]);
    pass.set_bind_group(1, settings, &[]);
    pass.draw(0..3, instance..instance + 1);
}

fn create_buffer(
    device: &wgpu::Device,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

fn fit_round(round: Round, width: f32, height: f32) -> Round {
    let mut radii = [round.lt, round.rt, round.rb, round.lb].map(|radius| radius.max(0.0));
    let edge_scale = |length: f32, radii: f32| {
        if length > 0.0 && radii > length {
            length / radii
        } else {
            1.0
        }
    };
    let scale = [
        edge_scale(width, radii[0] + radii[1]),
        edge_scale(height, radii[1] + radii[2]),
        edge_scale(width, radii[2] + radii[3]),
        edge_scale(height, radii[3] + radii[0]),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    radii.iter_mut().for_each(|radius| *radius *= scale);
    Round {
        lt: radii[0],
        rt: radii[1],
        rb: radii[2],
        lb: radii[3],
    }
}

const LIQUID_GLASS_SHADER_PREFIX: &str = r#"
struct WgslInput {
    local_position: vec2<f32>,
    local_uv: vec2<f32>,
    pixel_position: vec2<f32>,
    screen_uv: vec2<f32>,
    bounds: vec4<f32>,
};

struct LiquidGlassScreen {
    size: vec2<f32>,
    padding: vec2<f32>,
};

struct LiquidGlassClipMask {
    bounds: vec4<f32>,
    round: vec4<f32>,
};

struct LiquidGlassInstance {
    bounds: vec4<f32>,
    round: vec4<f32>,
    clip_range: vec2<u32>,
    padding: vec2<u32>,
    params: array<vec4<f32>, 4>,
};

@group(0) @binding(0)
var liquid_backdrop_texture: texture_2d<f32>;

@group(0) @binding(1)
var liquid_backdrop_sampler: sampler;

@group(1) @binding(0)
var<uniform> liquid_screen: LiquidGlassScreen;

@group(1) @binding(1)
var<storage, read> liquid_clip_masks: array<LiquidGlassClipMask>;

@group(1) @binding(2)
var<storage, read> liquid_instances: array<LiquidGlassInstance>;

fn sample_backdrop(screen_uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(
        liquid_backdrop_texture,
        liquid_backdrop_sampler,
        clamp(screen_uv, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    );
}
"#;

const LIQUID_GLASS_SHADER_SUFFIX: &str = r#"
struct LiquidVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local_position: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) pixel_position: vec2<f32>,
    @location(3) bounds: vec4<f32>,
    @location(4) @interpolate(flat) instance_index: u32,
};

@vertex
fn liquid_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> LiquidVertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let instance = liquid_instances[instance_index];
    let local_uv = corners[vertex_index];
    let local_position = local_uv * instance.bounds.zw;
    let pixel_position = instance.bounds.xy + local_position;
    let clip_position = vec2<f32>(
        pixel_position.x / liquid_screen.size.x * 2.0 - 1.0,
        1.0 - pixel_position.y / liquid_screen.size.y * 2.0,
    );
    return LiquidVertexOutput(
        vec4<f32>(clip_position, 0.0, 1.0),
        local_position,
        local_uv,
        pixel_position,
        instance.bounds,
        instance_index,
    );
}

fn liquid_clip_distance(
    position: vec2<f32>,
    bounds: vec4<f32>,
    round: vec4<f32>,
) -> f32 {
    let centered = position - bounds.xy - bounds.zw * 0.5;
    let top_radius = select(round.x, round.y, centered.x > 0.0);
    let bottom_radius = select(round.w, round.z, centered.x > 0.0);
    let radius = select(top_radius, bottom_radius, centered.y > 0.0);
    let corner = abs(centered) - bounds.zw * 0.5 + vec2<f32>(radius);
    return min(max(corner.x, corner.y), 0.0)
        + length(max(corner, vec2<f32>(0.0))) - radius;
}

fn liquid_coverage(distance: f32) -> f32 {
    let antialias_width = max(fwidth(distance), 0.75);
    return 1.0 - smoothstep(
        -antialias_width * 0.5,
        antialias_width * 0.5,
        distance,
    );
}

@fragment
fn liquid_fragment(input: LiquidVertexOutput) -> @location(0) vec4<f32> {
    let instance = liquid_instances[input.instance_index];
    var mask = 1.0;
    for (var index = 0u; index < instance.clip_range.y; index += 1u) {
        let clip_mask = liquid_clip_masks[instance.clip_range.x + index];
        mask *= liquid_coverage(liquid_clip_distance(
            input.pixel_position,
            clip_mask.bounds,
            clip_mask.round,
        ));
    }
    let user_input = WgslInput(
        input.local_position,
        input.local_uv,
        input.pixel_position,
        input.pixel_position / liquid_screen.size,
        input.bounds,
    );
    let affected = backdrop_main(user_input, instance.params);
    let alpha = affected.a * mask;
    return vec4<f32>(affected.rgb * alpha, alpha);
}
"#;

const GAUSSIAN_BLUR_WGSL: &str = r#"
@group(0) @binding(0)
var blur_source: texture_2d<f32>;

@group(0) @binding(1)
var blur_sampler: sampler;

@group(1) @binding(0)
var<storage, read> blur_settings: array<vec4<f32>>;

struct BlurVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) draw_index: u32,
};

@vertex
fn blur_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> BlurVertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return BlurVertexOutput(
        vec4<f32>(positions[vertex_index], 0.0, 1.0),
        instance_index,
    );
}

fn mirror_coordinate(value: i32, size: i32) -> i32 {
    let period = size * 2;
    let wrapped = ((value % period) + period) % period;
    return select(wrapped, period - wrapped - 1, wrapped >= size);
}

fn gaussian_blur(position: vec2<i32>, draw_index: u32, axis: vec2<i32>) -> vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(blur_source));
    let sigma = max(blur_settings[draw_index].x, 0.001);
    let radius = i32(ceil(sigma * 3.0));
    var result = vec4<f32>(0.0);
    var total_weight = 0.0;
    for (var offset = -radius; offset <= radius; offset += 1) {
        let sample_position = position + axis * offset;
        let mirrored = vec2<i32>(
            mirror_coordinate(sample_position.x, dimensions.x),
            mirror_coordinate(sample_position.y, dimensions.y),
        );
        let scalar_offset = f32(offset);
        let weight = exp(
            -(scalar_offset * scalar_offset) / (2.0 * sigma * sigma),
        );
        result += textureLoad(blur_source, mirrored, 0) * weight;
        total_weight += weight;
    }
    return result / total_weight;
}

@fragment
fn blur_horizontal(input: BlurVertexOutput) -> @location(0) vec4<f32> {
    return gaussian_blur(vec2<i32>(input.position.xy), input.draw_index, vec2<i32>(1, 0));
}

@fragment
fn blur_vertical(input: BlurVertexOutput) -> @location(0) vec4<f32> {
    return gaussian_blur(vec2<i32>(input.position.xy), input.draw_index, vec2<i32>(0, 1));
}
"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("render-shapes.png"));
    let liquid_glass = LiquidGlass::new();
    let mut surface = OffscreenSurface::new(SIZE).await.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no compatible WebGPU adapter found",
        )
    })?;
    let draws = scene(&liquid_glass);
    let rgba = surface.render_rgba8(&draws, color(9, 15, 32)).await;

    let file = File::create(&output)?;
    PngEncoder::new(file).write_image(&rgba, SIZE[0], SIZE[1], ExtendedColorType::Rgba8)?;

    println!("wrote {}", output.display());
    Ok(())
}

fn scene(liquid_glass: &LiquidGlass) -> DrawList {
    let mut draws = DrawList::new();
    let card = Rect::new(55.0, 40.0, 690.0, 420.0);
    let card_round = rounded(44.0);
    let glass = Rect::new(120.0, 135.0, 560.0, 230.0);
    let glass_round = rounded(58.0);

    draws.draw_rounded_rect(card, color(226, 232, 255), card_round);
    draws.with_rounded_clip(card, card_round, |draws| {
        draws.draw_rounded_rect(
            Rect::new(20.0, 75.0, 300.0, 300.0),
            color(99, 102, 241),
            rounded(150.0),
        );
        draws.draw_rounded_rect(
            Rect::new(500.0, 40.0, 300.0, 300.0),
            color(45, 212, 191),
            rounded(150.0),
        );
        draws.draw_rounded_rect(
            Rect::new(500.0, 315.0, 190.0, 190.0),
            color(251, 191, 36),
            rounded(95.0),
        );
        draws.draw_rounded_rect(
            Rect::new(195.0, 315.0, 210.0, 210.0),
            color(244, 114, 182),
            rounded(105.0),
        );

        let bar_colors = [
            color(244, 63, 94),
            color(56, 189, 248),
            color(167, 139, 250),
            color(251, 146, 60),
        ];
        for index in 0..9 {
            draws.draw_rounded_rect(
                Rect::new(98.0 + index as f32 * 72.0, 85.0, 28.0, 330.0),
                bar_colors[index % bar_colors.len()],
                rounded(14.0),
            );
        }
    });

    // A backdrop draw samples every command above it in the retained list.
    liquid_glass.draw_rounded(
        &mut draws,
        glass,
        glass_round,
        5.0,
        [
            [1.0, 1.0, 1.0, 0.0],
            [1.2, 0.01, 20.0, 0.0],
            [0.0, 1.0, 0.5, 0.0],
            [SIZE[0] as f32, SIZE[1] as f32, 1.5, 58.0],
        ],
    );
    // This opaque pill is recorded later, so it stays crisp on top of the glass.
    draws.draw_rounded_rect(
        Rect::new(285.0, 211.0, 230.0, 78.0),
        color(15, 23, 42),
        rounded(39.0),
    );
    draws.draw_rounded_stroke(
        Stroke::new(285.0, 211.0, 230.0, 78.0, 2.0),
        rgba(255, 255, 255, 0.9),
        rounded(39.0),
    );
    draws.draw_rounded_stroke(
        Stroke::from_rect(card, 3.0),
        rgba(255, 255, 255, 0.6),
        card_round,
    );

    draws
}

const fn rounded(radius: f32) -> Round {
    Round {
        lt: radius,
        rt: radius,
        rb: radius,
        lb: radius,
    }
}

const fn color(red: u8, green: u8, blue: u8) -> wgpu::Color {
    rgba(red, green, blue, 1.0)
}

const fn rgba(red: u8, green: u8, blue: u8, alpha: f64) -> wgpu::Color {
    wgpu::Color {
        r: red as f64 / 255.0,
        g: green as f64 / 255.0,
        b: blue as f64 / 255.0,
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_renderer_shaders_are_valid_wgsl() {
        let effect = format!(
            "{}\n{}\n{}",
            LIQUID_GLASS_SHADER_PREFIX, LIQUID_GLASS_WGSL, LIQUID_GLASS_SHADER_SUFFIX
        );
        validate_wgsl(&effect);
        validate_wgsl(GAUSSIAN_BLUR_WGSL);
    }

    fn validate_wgsl(source: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("shader must pass Naga validation");
    }
}
