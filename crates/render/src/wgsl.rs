//! Ready-made typed renderers for small custom WGSL effects.
//!
//! The engine owns geometry, clip evaluation, and resource bindings. Custom
//! source only supplies a `raster_main` or `backdrop_main` function, while each
//! recorded draw carries a fixed number of `vec4<f32>` parameters.

use std::{ops::Range, sync::Arc};

use crate::{
    backdrop::{
        BackdropCreateContext, BackdropPrepareContext, BackdropRenderer, BackdropRendererFactory,
        BackdropRendererHandle, ResolvedBackdropDraw,
    },
    canvas::DrawList,
    clip::ClipMask,
    raster::{
        ClipMaskRange, RasterBatch, RasterCreateContext, RasterPrepareContext, RasterRenderer,
        RasterRendererFactory, RasterRendererHandle, ResolvedRasterDraw,
    },
    shapes::{rect::Rect, round::Round},
};

const SCREEN_UNIFORM_SIZE: u64 = 16;
const CLIP_MASK_SIZE: u64 = std::mem::size_of::<ClipMask>() as u64;
const INSTANCE_PREFIX_SIZE: usize = 48;

/// One bounded custom-shader draw.
///
/// Parameters map exactly to WGSL's `array<vec4<f32>, N>`. Keeping this ABI
/// deliberately small makes Rust/WGSL layout deterministic and avoids unsafe
/// user-defined uniform layouts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WgslDraw<const N: usize = 1> {
    pub bounds: Rect,
    pub round: Round,
    pub params: [[f32; 4]; N],
}

/// A custom WGSL source or parameter-layout error.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WgslError {
    #[error("WGSL shaders require at least one vec4 parameter")]
    EmptyParameters,
    #[error("WGSL parameter count is too large")]
    ParameterCountTooLarge,
    #[error("invalid WGSL shader: {0}")]
    InvalidShader(String),
}

/// A reusable custom raster shader.
///
/// `source` must define exactly the callable entry point below. The engine
/// supplies `WgslInput`, vertex and fragment entry points, rounded bounds,
/// central clips, and straight-alpha blending.
///
/// ```wgsl
/// fn raster_main(
///     input: WgslInput,
///     params: array<vec4<f32>, 1>,
/// ) -> vec4<f32> {
///     return params[0];
/// }
/// ```
#[derive(Clone, Debug)]
pub struct WgslRaster<const N: usize = 1> {
    handle: RasterRendererHandle<WgslDraw<N>>,
}

impl<const N: usize> WgslRaster<N> {
    /// Validates and registers a custom raster shader.
    pub fn new(label: impl Into<Arc<str>>, source: impl AsRef<str>) -> Result<Self, WgslError> {
        validate_parameter_count::<N>()?;
        let label = label.into();
        let source: Arc<str> = compose_raster_source::<N>(source.as_ref()).into();
        validate_shader(&source)?;
        let factory = WgslRasterFactory {
            label: Arc::clone(&label),
            source,
        };
        Ok(Self {
            handle: RasterRendererHandle::new(label, factory),
        })
    }

    pub const fn handle(&self) -> &RasterRendererHandle<WgslDraw<N>> {
        &self.handle
    }

    /// Records one rectangular custom raster draw.
    pub fn draw<'draws>(
        &self,
        draws: &'draws mut DrawList,
        bounds: Rect,
        params: [[f32; 4]; N],
    ) -> &'draws mut DrawList {
        self.draw_rounded(draws, bounds, Round::default(), params)
    }

    /// Records one rounded custom raster draw.
    pub fn draw_rounded<'draws>(
        &self,
        draws: &'draws mut DrawList,
        bounds: Rect,
        round: Round,
        params: [[f32; 4]; N],
    ) -> &'draws mut DrawList {
        draws.draw_with(
            &self.handle,
            WgslDraw {
                bounds,
                round,
                params,
            },
        )
    }
}

/// A reusable custom effect that samples the scene produced by earlier draws.
///
/// `source` must define `backdrop_main` with the same input and parameter ABI
/// as [`WgslRaster`]. It may call `sample_backdrop(screen_uv)` to sample the
/// prior scene. Its return value is the final affected pixel; the engine mixes
/// it with the unmodified prior pixel at rounded and clipped edges.
#[derive(Clone, Debug)]
pub struct WgslBackdrop<const N: usize = 1> {
    handle: BackdropRendererHandle<WgslDraw<N>>,
}

impl<const N: usize> WgslBackdrop<N> {
    /// Validates and registers a custom backdrop shader.
    pub fn new(label: impl Into<Arc<str>>, source: impl AsRef<str>) -> Result<Self, WgslError> {
        validate_parameter_count::<N>()?;
        let label = label.into();
        let source: Arc<str> = compose_backdrop_source::<N>(source.as_ref()).into();
        validate_shader(&source)?;
        let factory = WgslBackdropFactory {
            label: Arc::clone(&label),
            source,
        };
        Ok(Self {
            handle: BackdropRendererHandle::new(label, factory),
        })
    }

    pub const fn handle(&self) -> &BackdropRendererHandle<WgslDraw<N>> {
        &self.handle
    }

    /// Records one rectangular backdrop effect.
    pub fn draw<'draws>(
        &self,
        draws: &'draws mut DrawList,
        bounds: Rect,
        params: [[f32; 4]; N],
    ) -> &'draws mut DrawList {
        self.draw_rounded(draws, bounds, Round::default(), params)
    }

    /// Records one rounded backdrop effect.
    pub fn draw_rounded<'draws>(
        &self,
        draws: &'draws mut DrawList,
        bounds: Rect,
        round: Round,
        params: [[f32; 4]; N],
    ) -> &'draws mut DrawList {
        draws.draw(self.handle.command(WgslDraw {
            bounds,
            round,
            params,
        }))
    }
}

#[derive(Clone)]
struct WgslRasterFactory<const N: usize> {
    label: Arc<str>,
    source: Arc<str>,
}

impl<const N: usize> RasterRendererFactory<WgslDraw<N>> for WgslRasterFactory<N> {
    type Renderer = WgslRasterRenderer<N>;

    fn create(&self, context: RasterCreateContext<'_>) -> Self::Renderer {
        WgslRasterRenderer::new(context, &self.label, &self.source)
    }
}

struct WgslRasterRenderer<const N: usize> {
    pipeline: wgpu::RenderPipeline,
    buffers: WgslBuffers,
    instance_bytes: Vec<u8>,
    batches: Vec<Range<u32>>,
}

impl<const N: usize> WgslRasterRenderer<N> {
    fn new(context: RasterCreateContext<'_>, label: &str, source: &str) -> Self {
        let buffers = WgslBuffers::new(context.device, label, instance_stride::<N>() as u64);
        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = create_pipeline(
            context.device,
            label,
            &shader,
            context.target_format,
            context.sample_count,
            &[Some(&buffers.bind_group_layout)],
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        Self {
            pipeline,
            buffers,
            instance_bytes: Vec::new(),
            batches: Vec::new(),
        }
    }
}

impl<const N: usize> RasterRenderer<WgslDraw<N>> for WgslRasterRenderer<N> {
    fn prepare(
        &mut self,
        context: RasterPrepareContext<'_>,
        draws: &[ResolvedRasterDraw<'_, WgslDraw<N>>],
        batches: &[RasterBatch],
    ) {
        self.instance_bytes.clear();
        self.batches.clear();

        let mut instance_count = 0_u32;
        for batch in batches {
            let first_instance = instance_count;
            for draw_index in batch.draws.clone() {
                let draw = &draws[draw_index];
                if append_instance(&mut self.instance_bytes, draw.payload, draw.clip_masks) {
                    instance_count = instance_count
                        .checked_add(1)
                        .expect("a frame cannot contain more than u32::MAX WGSL draws");
                }
            }
            self.batches.push(first_instance..instance_count);
        }

        self.buffers.prepare(
            context.device,
            context.queue,
            context.canvas_size,
            context.clip_masks,
            &self.instance_bytes,
        );
    }

    fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, batch_index: usize) {
        let instances = &self.batches[batch_index];
        if instances.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.buffers.bind_group, &[]);
        pass.draw(0..6, instances.clone());
    }
}

#[derive(Clone)]
struct WgslBackdropFactory<const N: usize> {
    label: Arc<str>,
    source: Arc<str>,
}

impl<const N: usize> BackdropRendererFactory<WgslDraw<N>> for WgslBackdropFactory<N> {
    type Renderer = WgslBackdropRenderer<N>;

    fn create(&self, context: BackdropCreateContext<'_>) -> Self::Renderer {
        WgslBackdropRenderer::new(context, &self.label, &self.source)
    }
}

struct WgslBackdropRenderer<const N: usize> {
    pipeline: wgpu::RenderPipeline,
    buffers: WgslBuffers,
    instance_bytes: Vec<u8>,
    instance_indices: Vec<Option<u32>>,
}

impl<const N: usize> WgslBackdropRenderer<N> {
    fn new(context: BackdropCreateContext<'_>, label: &str, source: &str) -> Self {
        let buffers = WgslBuffers::new(context.device, label, instance_stride::<N>() as u64);
        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = create_pipeline(
            context.device,
            label,
            &shader,
            context.target_format,
            1,
            &[
                Some(context.scene_bind_group_layout),
                Some(&buffers.bind_group_layout),
            ],
            None,
        );
        Self {
            pipeline,
            buffers,
            instance_bytes: Vec::new(),
            instance_indices: Vec::new(),
        }
    }
}

impl<const N: usize> BackdropRenderer<WgslDraw<N>> for WgslBackdropRenderer<N> {
    fn prepare(
        &mut self,
        context: BackdropPrepareContext<'_>,
        draws: &[ResolvedBackdropDraw<'_, WgslDraw<N>>],
    ) {
        self.instance_bytes.clear();
        self.instance_indices.clear();

        let mut instance_count = 0_u32;
        for draw in draws {
            if append_instance(&mut self.instance_bytes, draw.payload, draw.clip_masks) {
                self.instance_indices.push(Some(instance_count));
                instance_count = instance_count
                    .checked_add(1)
                    .expect("a frame cannot contain more than u32::MAX WGSL draws");
            } else {
                self.instance_indices.push(None);
            }
        }

        self.buffers.prepare(
            context.device,
            context.queue,
            context.canvas_size,
            context.clip_masks,
            &self.instance_bytes,
        );
    }

    fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, draw_index: usize) {
        let Some(instance) = self.instance_indices[draw_index] else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, &self.buffers.bind_group, &[]);
        pass.draw(0..6, instance..instance + 1);
    }
}

struct WgslBuffers {
    label: Arc<str>,
    bind_group_layout: wgpu::BindGroupLayout,
    screen_buffer: wgpu::Buffer,
    clip_buffer: wgpu::Buffer,
    clip_capacity: u64,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    bind_group: wgpu::BindGroup,
}

impl WgslBuffers {
    fn new(device: &wgpu::Device, label: &str, initial_instance_size: u64) -> Self {
        let label: Arc<str> = label.into();
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label} WGSL resources")),
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
        });
        let screen_buffer = create_buffer(
            device,
            &format!("{label} WGSL screen uniform"),
            SCREEN_UNIFORM_SIZE,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let clip_capacity = CLIP_MASK_SIZE;
        let clip_buffer = create_buffer(
            device,
            &format!("{label} WGSL clip masks"),
            clip_capacity,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let instance_capacity = initial_instance_size;
        let instance_buffer = create_buffer(
            device,
            &format!("{label} WGSL instances"),
            instance_capacity,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let bind_group = create_resource_bind_group(
            device,
            &label,
            &bind_group_layout,
            &screen_buffer,
            &clip_buffer,
            &instance_buffer,
        );
        Self {
            label,
            bind_group_layout,
            screen_buffer,
            clip_buffer,
            clip_capacity,
            instance_buffer,
            instance_capacity,
            bind_group,
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas_size: [u32; 2],
        clip_masks: &[ClipMask],
        instance_bytes: &[u8],
    ) {
        queue.write_buffer(
            &self.screen_buffer,
            0,
            bytemuck::cast_slice(&[canvas_size[0] as f32, canvas_size[1] as f32, 0.0, 0.0]),
        );

        let required_clips = std::mem::size_of_val(clip_masks) as u64;
        let required_instances = instance_bytes.len() as u64;
        let mut recreate_bind_group = false;
        if required_clips > self.clip_capacity {
            self.clip_capacity = required_clips.next_power_of_two();
            self.clip_buffer = create_buffer(
                device,
                &format!("{} WGSL clip masks", self.label),
                self.clip_capacity,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            recreate_bind_group = true;
        }
        if required_instances > self.instance_capacity {
            self.instance_capacity = required_instances.next_power_of_two();
            self.instance_buffer = create_buffer(
                device,
                &format!("{} WGSL instances", self.label),
                self.instance_capacity,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            recreate_bind_group = true;
        }
        if recreate_bind_group {
            self.bind_group = create_resource_bind_group(
                device,
                &self.label,
                &self.bind_group_layout,
                &self.screen_buffer,
                &self.clip_buffer,
                &self.instance_buffer,
            );
        }

        if !clip_masks.is_empty() {
            queue.write_buffer(&self.clip_buffer, 0, bytemuck::cast_slice(clip_masks));
        }
        if !instance_bytes.is_empty() {
            queue.write_buffer(&self.instance_buffer, 0, instance_bytes);
        }
    }
}

fn append_instance<const N: usize>(
    bytes: &mut Vec<u8>,
    draw: &WgslDraw<N>,
    clip_masks: ClipMaskRange,
) -> bool {
    if draw.bounds.is_empty() {
        return false;
    }
    let round = draw.round.fit(draw.bounds.width, draw.bounds.height);
    extend_bytes(
        bytes,
        &[
            draw.bounds.x,
            draw.bounds.y,
            draw.bounds.width,
            draw.bounds.height,
        ],
    );
    extend_bytes(bytes, &[round.lt, round.rt, round.rb, round.lb]);
    extend_bytes(bytes, &clip_masks.as_array());
    extend_bytes(bytes, [0_u32; 2].as_slice());
    extend_bytes(bytes, &draw.params);
    debug_assert_eq!(bytes.len() % instance_stride::<N>(), 0);
    true
}

fn extend_bytes<T: bytemuck::NoUninit>(bytes: &mut Vec<u8>, values: &[T]) {
    bytes.extend_from_slice(bytemuck::cast_slice(values));
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

fn create_resource_bind_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    screen_buffer: &wgpu::Buffer,
    clip_buffer: &wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{label} WGSL resources")),
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

fn create_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{label} WGSL pipeline layout")),
        bind_group_layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("wgsl_vertex_main"),
            compilation_options: Default::default(),
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
            entry_point: Some("wgsl_fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn validate_parameter_count<const N: usize>() -> Result<(), WgslError> {
    if N == 0 {
        return Err(WgslError::EmptyParameters);
    }
    checked_instance_stride::<N>().ok_or(WgslError::ParameterCountTooLarge)?;
    Ok(())
}

fn instance_stride<const N: usize>() -> usize {
    checked_instance_stride::<N>().expect("validated WGSL parameter count must fit in usize")
}

fn checked_instance_stride<const N: usize>() -> Option<usize> {
    N.checked_mul(16)
        .and_then(|params| params.checked_add(INSTANCE_PREFIX_SIZE))
}

fn validate_shader(source: &str) -> Result<(), WgslError> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| WgslError::InvalidShader(error.emit_to_string(source)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| WgslError::InvalidShader(error.emit_to_string(source)))?;
    Ok(())
}

fn compose_raster_source<const N: usize>(source: &str) -> String {
    format!(
        "{}\n{source}\n{}",
        common_shader_prefix(),
        shader_suffix::<N>(0, "raster")
    )
}

fn compose_backdrop_source<const N: usize>(source: &str) -> String {
    format!(
        "{}\n{}\n{source}\n{}",
        common_shader_prefix(),
        BACKDROP_SHADER_PREFIX,
        shader_suffix::<N>(1, "backdrop")
    )
}

fn common_shader_prefix() -> &'static str {
    r#"
struct WgslInput {
    local_position: vec2<f32>,
    local_uv: vec2<f32>,
    pixel_position: vec2<f32>,
    screen_uv: vec2<f32>,
    bounds: vec4<f32>,
};
"#
}

const BACKDROP_SHADER_PREFIX: &str = r#"
@group(0) @binding(0)
var wgsl_backdrop_texture: texture_2d<f32>;

@group(0) @binding(1)
var wgsl_backdrop_sampler: sampler;

fn sample_backdrop(screen_uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(
        wgsl_backdrop_texture,
        wgsl_backdrop_sampler,
        clamp(screen_uv, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    );
}
"#;

fn shader_suffix<const N: usize>(resource_group: u32, kind: &str) -> String {
    let effect_call = if kind == "raster" {
        "let affected = raster_main(user_input, instance.params);\n    return vec4<f32>(affected.rgb, affected.a * mask);"
    } else {
        "let prior = sample_backdrop(user_input.screen_uv);\n    let affected = backdrop_main(user_input, instance.params);\n    return mix(prior, affected, mask);"
    };
    format!(
        r#"
struct WgslScreen {{
    size: vec2<f32>,
    padding: vec2<f32>,
}};

struct WgslClipMask {{
    bounds: vec4<f32>,
    round: vec4<f32>,
}};

struct WgslInstance {{
    bounds: vec4<f32>,
    round: vec4<f32>,
    clip_range: vec2<u32>,
    padding: vec2<u32>,
    params: array<vec4<f32>, {N}>,
}};

@group({resource_group}) @binding(0)
var<uniform> wgsl_screen: WgslScreen;

@group({resource_group}) @binding(1)
var<storage, read> wgsl_clip_masks: array<WgslClipMask>;

@group({resource_group}) @binding(2)
var<storage, read> wgsl_instances: array<WgslInstance>;

struct WgslVertexOutput {{
    @builtin(position) position: vec4<f32>,
    @location(0) local_position: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) pixel_position: vec2<f32>,
    @location(3) bounds: vec4<f32>,
    @location(4) @interpolate(flat) instance_index: u32,
}};

@vertex
fn wgsl_vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> WgslVertexOutput {{
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let instance = wgsl_instances[instance_index];
    let local_uv = corners[vertex_index];
    let local_position = local_uv * instance.bounds.zw;
    let pixel_position = instance.bounds.xy + local_position;
    let clip_position = vec2<f32>(
        pixel_position.x / wgsl_screen.size.x * 2.0 - 1.0,
        1.0 - pixel_position.y / wgsl_screen.size.y * 2.0,
    );
    return WgslVertexOutput(
        vec4<f32>(clip_position, 0.0, 1.0),
        local_position,
        local_uv,
        pixel_position,
        instance.bounds,
        instance_index,
    );
}}

fn wgsl_rounded_distance(
    position: vec2<f32>,
    bounds: vec4<f32>,
    round: vec4<f32>,
) -> f32 {{
    let centered = position - bounds.xy - bounds.zw * 0.5;
    let top_radius = select(round.x, round.y, centered.x > 0.0);
    let bottom_radius = select(round.w, round.z, centered.x > 0.0);
    let radius = select(top_radius, bottom_radius, centered.y > 0.0);
    let corner = abs(centered) - bounds.zw * 0.5 + vec2<f32>(radius);
    return min(max(corner.x, corner.y), 0.0)
        + length(max(corner, vec2<f32>(0.0))) - radius;
}}

fn wgsl_coverage(distance: f32) -> f32 {{
    let antialias_width = max(fwidth(distance), 0.75);
    return 1.0 - smoothstep(
        -antialias_width * 0.5,
        antialias_width * 0.5,
        distance,
    );
}}

@fragment
fn wgsl_fragment_main(input: WgslVertexOutput) -> @location(0) vec4<f32> {{
    let instance = wgsl_instances[input.instance_index];
    let user_input = WgslInput(
        input.local_position,
        input.local_uv,
        input.pixel_position,
        input.pixel_position / wgsl_screen.size,
        input.bounds,
    );
    var mask = wgsl_coverage(wgsl_rounded_distance(
        input.pixel_position,
        instance.bounds,
        instance.round,
    ));
    for (var index = 0u; index < instance.clip_range.y; index += 1u) {{
        let clip_mask = wgsl_clip_masks[instance.clip_range.x + index];
        mask *= wgsl_coverage(wgsl_rounded_distance(
            input.pixel_position,
            clip_mask.bounds,
            clip_mask.round,
        ));
    }}
    {effect_call}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RASTER_SOURCE: &str = r#"
fn raster_main(input: WgslInput, params: array<vec4<f32>, 1>) -> vec4<f32> {
    return params[0] + vec4<f32>(input.local_uv * 0.0, 0.0, 0.0);
}
"#;

    const BACKDROP_SOURCE: &str = r#"
fn backdrop_main(input: WgslInput, params: array<vec4<f32>, 2>) -> vec4<f32> {
    let prior = sample_backdrop(input.screen_uv);
    return vec4<f32>(mix(prior.rgb, vec3<f32>(1.0) - prior.rgb, params[0].x), prior.a)
        * params[1];
}
"#;

    #[test]
    fn accepts_the_documented_raster_and_backdrop_contracts() {
        assert!(WgslRaster::<1>::new("solid raster", RASTER_SOURCE).is_ok());
        assert!(WgslBackdrop::<2>::new("invert backdrop", BACKDROP_SOURCE).is_ok());
    }

    #[test]
    fn rejects_zero_parameters_and_invalid_entry_points() {
        assert_eq!(
            WgslRaster::<0>::new("empty", RASTER_SOURCE).unwrap_err(),
            WgslError::EmptyParameters
        );
        let error = WgslRaster::<1>::new(
            "missing raster main",
            "fn something_else() -> vec4<f32> { return vec4<f32>(1.0); }",
        )
        .unwrap_err();
        assert!(matches!(error, WgslError::InvalidShader(_)));
    }
}
