//! Internal scene textures and fullscreen presentation passes.
//!
//! Raster work is accumulated into a single-sample scene texture. Backdrop
//! effects read one scene while writing its ping-pong peer, which keeps WebGPU
//! from seeing the same texture as both a sampled resource and an attachment
//! in one render pass.

use crate::engine::RenderTarget;

const SCENE_COUNT: usize = 2;

/// Persistent pipelines and lazily sized textures used to compose one frame.
pub(crate) struct SceneCompositor {
    format: wgpu::TextureFormat,
    sample_count: u32,
    scene_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    single_sample_blit_pipeline: wgpu::RenderPipeline,
    multisample_blit_pipeline: Option<wgpu::RenderPipeline>,
    resources: Option<SceneResources>,
}

struct SceneResources {
    size: [u32; 2],
    scenes: [Scene; SCENE_COUNT],
    multisample_work: Option<MultisampleWork>,
}

struct Scene {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    source_bind_group: wgpu::BindGroup,
}

struct MultisampleWork {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl SceneCompositor {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        assert!(sample_count > 0, "scene sample count must be non-zero");

        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("render scene source bind group layout"),
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
                ],
            });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("render scene sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render scene fullscreen blit shader"),
            source: wgpu::ShaderSource::Wgsl(FULLSCREEN_BLIT_WGSL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render scene fullscreen blit pipeline layout"),
            bind_group_layouts: &[Some(&scene_bind_group_layout)],
            immediate_size: 0,
        });
        let single_sample_blit_pipeline = create_blit_pipeline(
            device,
            &pipeline_layout,
            &shader,
            format,
            1,
            "render scene single-sample blit pipeline",
        );
        let multisample_blit_pipeline = (sample_count > 1).then(|| {
            create_blit_pipeline(
                device,
                &pipeline_layout,
                &shader,
                format,
                sample_count,
                "render scene multisample blit pipeline",
            )
        });

        Self {
            format,
            sample_count,
            scene_bind_group_layout,
            sampler,
            single_sample_blit_pipeline,
            multisample_blit_pipeline,
            resources: None,
        }
    }

    /// Layout used by backdrop shaders to read the scene at group zero.
    pub(crate) fn scene_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.scene_bind_group_layout
    }

    pub(crate) const fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Creates or replaces the frame-sized scene textures.
    pub(crate) fn ensure_size(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        assert!(
            size[0] > 0 && size[1] > 0,
            "scene textures must have a non-zero size"
        );
        if self
            .resources
            .as_ref()
            .is_some_and(|resources| resources.size == size)
        {
            return;
        }

        let first = create_scene(
            device,
            &self.scene_bind_group_layout,
            &self.sampler,
            self.format,
            size,
            0,
        );
        let second = create_scene(
            device,
            &self.scene_bind_group_layout,
            &self.sampler,
            self.format,
            size,
            1,
        );
        let multisample_work = (self.sample_count > 1)
            .then(|| create_multisample_work(device, self.format, self.sample_count, size));
        self.resources = Some(SceneResources {
            size,
            scenes: [first, second],
            multisample_work,
        });
    }

    /// Clears one single-sample scene, normally scene zero at frame start.
    pub(crate) fn clear_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene_index: usize,
        color: wgpu::Color,
    ) {
        let scene = self.scene(scene_index);
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render scene clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &scene.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    /// Begins a raster run with all pixels initialized from `source_index`.
    ///
    /// With MSAA, the returned pass targets the transient work texture and
    /// resolves into `destination_index`. The fullscreen seed draw and caller's
    /// raster draws deliberately share one pass so multisample contents never
    /// need to be copied or preserved between passes.
    pub(crate) fn begin_raster_run<'pass>(
        &'pass self,
        encoder: &'pass mut wgpu::CommandEncoder,
        source_index: usize,
        destination_index: usize,
    ) -> wgpu::RenderPass<'pass> {
        let source = self.scene(source_index);
        let destination = self.scene(destination_index);

        if self.sample_count == 1 && source_index == destination_index {
            return encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render scene single-sample raster pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &destination.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        assert_ne!(
            source_index, destination_index,
            "a sampled scene cannot also be the raster resolve target"
        );
        let (view, resolve_target, store, pipeline, label) = if self.sample_count == 1 {
            (
                &destination.view,
                None,
                wgpu::StoreOp::Store,
                &self.single_sample_blit_pipeline,
                "render scene seeded single-sample raster pass",
            )
        } else {
            let work = self
                .resources()
                .multisample_work
                .as_ref()
                .expect("multisample scene work texture must exist");
            (
                &work.view,
                Some(&destination.view),
                wgpu::StoreOp::Discard,
                self.multisample_blit_pipeline
                    .as_ref()
                    .expect("multisample scene blit pipeline must exist"),
                "render scene seeded multisample raster pass",
            )
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &source.source_bind_group, &[]);
        pass.draw(0..3, 0..1);
        pass
    }

    /// Copies the current scene and begins a sample-one backdrop pass on its
    /// peer. The caller samples `source_index` while drawing into the returned
    /// pass; untouched and translucent pixels retain the copied scene.
    pub(crate) fn begin_backdrop<'pass>(
        &'pass self,
        encoder: &'pass mut wgpu::CommandEncoder,
        source_index: usize,
        destination_index: usize,
    ) -> wgpu::RenderPass<'pass> {
        assert_ne!(
            source_index, destination_index,
            "a backdrop must read and write different scene textures"
        );
        let source = self.scene(source_index);
        let destination = self.scene(destination_index);
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &destination.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.extent(),
        );

        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render scene backdrop pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &destination.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Bind group that exposes one scene texture and the compositor sampler.
    pub(crate) fn scene_source_bind_group(&self, scene_index: usize) -> &wgpu::BindGroup {
        &self.scene(scene_index).source_bind_group
    }

    /// Blits the composed scene into the adapter-owned frame target.
    pub(crate) fn present(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene_index: usize,
        target: RenderTarget<'_>,
    ) {
        if self.sample_count == 1 {
            assert!(
                target.resolve_view.is_none(),
                "a single-sample target cannot have a resolve view"
            );
        } else {
            assert!(
                target.resolve_view.is_some(),
                "a multisample target requires a resolve view"
            );
        }

        let scene = self.scene(scene_index);
        let pipeline = self
            .multisample_blit_pipeline
            .as_ref()
            .unwrap_or(&self.single_sample_blit_pipeline);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render scene presentation pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.color_view,
                depth_slice: None,
                resolve_target: target.resolve_view,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: target.store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &scene.source_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn resources(&self) -> &SceneResources {
        self.resources
            .as_ref()
            .expect("scene textures must be sized before encoding")
    }

    fn scene(&self, index: usize) -> &Scene {
        &self.resources().scenes[index]
    }

    fn extent(&self) -> wgpu::Extent3d {
        let [width, height] = self.resources().size;
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        }
    }
}

fn create_scene(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    index: usize,
) -> Scene {
    let texture_label = match index {
        0 => "render scene texture 0",
        1 => "render scene texture 1",
        _ => unreachable!("the compositor owns exactly two scenes"),
    };
    let view_label = match index {
        0 => "render scene texture view 0",
        1 => "render scene texture view 1",
        _ => unreachable!("the compositor owns exactly two scenes"),
    };
    let bind_group_label = match index {
        0 => "render scene source bind group 0",
        1 => "render scene source bind group 1",
        _ => unreachable!("the compositor owns exactly two scenes"),
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(texture_label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(view_label),
        ..Default::default()
    });
    let source_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(bind_group_label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Scene {
        texture,
        view,
        source_bind_group,
    }
}

fn create_multisample_work(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sample_count: u32,
    size: [u32; 2],
) -> MultisampleWork {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("render scene multisample work texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("render scene multisample work texture view"),
        ..Default::default()
    });
    MultisampleWork {
        _texture: texture,
        view,
    }
}

fn create_blit_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    sample_count: u32,
    label: &'static str,
) -> wgpu::RenderPipeline {
    let targets = [Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

const FULLSCREEN_BLIT_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0)
var scene: texture_2d<f32>;

@group(0) @binding(1)
var scene_sampler: sampler;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let uv = vec2<f32>(
        f32((vertex_index << 1u) & 2u),
        f32(vertex_index & 2u),
    );
    return VertexOutput(
        vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0),
        uv,
    );
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSampleLevel(scene, scene_sampler, input.uv, 0.0);
}
"#;
