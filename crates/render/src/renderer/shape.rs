use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{
    BackgroundImage, Border, BoxStyle, Canvas, Clip, Color, DrawCommand, RasterImage, Rect,
    Transform,
};

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
    image_texture: wgpu::Texture,
    image_view: wgpu::TextureView,
    image_sampler: wgpu::Sampler,
    image_extent: [u32; 3],
    images: Vec<RasterImage>,
    instances: Vec<ShapeInstance>,
    clips: Vec<GpuClip>,
    overlay_start: usize,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("render background image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let (image_texture, image_view) = create_image_texture(device, [1, 1, 1]);
        let screen_bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &screen_buffer,
            &clip_buffer,
            &image_view,
            &image_sampler,
        );
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
            image_texture,
            image_view,
            image_sampler,
            image_extent: [1, 1, 1],
            images: Vec::new(),
            instances: Vec::new(),
            clips: Vec::new(),
            overlay_start: 0,
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas: &Canvas,
        size: SurfaceSize,
    ) {
        self.prepare_images(device, queue, canvas);
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
        for overlay in [false, true] {
            if overlay {
                self.overlay_start = self.instances.len();
            }
            for command in canvas.commands() {
                let shape = match command {
                    DrawCommand::Box { rect, style, clips } if !overlay => {
                        Some((*rect, style.clone(), clips.as_slice()))
                    }
                    DrawCommand::OverlayBox { rect, style, clips } if overlay => {
                        Some((*rect, style.clone(), clips.as_slice()))
                    }
                    _ => None,
                };
                let Some((rect, style, clips)) = shape else {
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
                let image_layer = match &style.background_image {
                    Some(BackgroundImage::Raster(image)) => self
                        .images
                        .iter()
                        .position(|candidate| candidate == image)
                        .map(|layer| {
                            (
                                layer as u32,
                                [
                                    image.width as f32 / self.image_extent[0] as f32,
                                    image.height as f32 / self.image_extent[1] as f32,
                                ],
                            )
                        }),
                    _ => None,
                };
                if let Some(shadow) = style.shadow {
                    let spread = shadow.spread;
                    let shadow_rect = Rect::new(
                        rect.x + shadow.offset[0] - spread,
                        rect.y + shadow.offset[1] - spread,
                        rect.width + spread * 2.0,
                        rect.height + spread * 2.0,
                    );
                    let mut shadow_style = BoxStyle {
                        background: shadow.color,
                        corner_radius: style.corner_radius,
                        opacity: style.opacity,
                        transform: style.transform,
                        ..BoxStyle::default()
                    };
                    let radius_spread = spread.max(0.0);
                    for corner in [
                        &mut shadow_style.corner_radius.top_left,
                        &mut shadow_style.corner_radius.top_right,
                        &mut shadow_style.corner_radius.bottom_right,
                        &mut shadow_style.corner_radius.bottom_left,
                    ] {
                        corner.x += radius_spread;
                        corner.y += radius_spread;
                    }
                    self.instances.push(ShapeInstance::new(
                        shadow_rect,
                        shadow_style,
                        clip_start,
                        clips.len() as u32,
                        shadow.blur,
                        None,
                    ));
                }
                self.instances.push(ShapeInstance::new(
                    rect,
                    style,
                    clip_start,
                    clips.len() as u32,
                    0.0,
                    image_layer,
                ));
            }
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
                    &self.image_view,
                    &self.image_sampler,
                );
            }
            queue.write_buffer(&self.clip_buffer, 0, clip_bytes);
        }
    }

    fn prepare_images(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, canvas: &Canvas) {
        self.images.clear();
        for image in canvas.commands().iter().filter_map(|command| {
            let style = match command {
                DrawCommand::Box { style, .. } | DrawCommand::OverlayBox { style, .. } => style,
                DrawCommand::Text { .. } => return None,
            };
            match &style.background_image {
                Some(BackgroundImage::Raster(image)) => Some(image),
                _ => None,
            }
        }) {
            if !self.images.iter().any(|candidate| candidate == image) {
                self.images.push(image.clone());
            }
        }
        let extent = [
            self.images
                .iter()
                .map(|image| image.width)
                .max()
                .unwrap_or(1),
            self.images
                .iter()
                .map(|image| image.height)
                .max()
                .unwrap_or(1),
            self.images.len().max(1) as u32,
        ];
        if extent != self.image_extent {
            (self.image_texture, self.image_view) = create_image_texture(device, extent);
            self.image_extent = extent;
            self.screen_bind_group = create_bind_group(
                device,
                &self.bind_group_layout,
                &self.screen_buffer,
                &self.clip_buffer,
                &self.image_view,
                &self.image_sampler,
            );
        }
        for (layer, image) in self.images.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.image_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &image.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * 4),
                    rows_per_image: Some(image.height),
                },
                wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn draw_base<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.draw_range(pass, 0, self.overlay_start);
    }

    pub fn draw_overlay<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.draw_range(pass, self.overlay_start, self.instances.len());
    }

    fn draw_range<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        start: usize,
        end: usize,
    ) {
        if start >= end {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..6, start as u32..end as u32);
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    screen_buffer: &wgpu::Buffer,
    clip_buffer: &wgpu::Buffer,
    image_view: &wgpu::TextureView,
    image_sampler: &wgpu::Sampler,
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
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(image_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(image_sampler),
            },
        ],
    })
}

fn create_image_texture(
    device: &wgpu::Device,
    extent: [u32; 3],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("render background image array"),
        size: wgpu::Extent3d {
            width: extent[0],
            height: extent[1],
            depth_or_array_layers: extent[2],
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("render background image array view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..wgpu::TextureViewDescriptor::default()
    });
    (texture, view)
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
    radii_x: [f32; 4],
    radii_y: [f32; 4],
    background: [f32; 4],
    border_colors: [u32; 4],
    outline_color: [f32; 4],
    border_widths: [f32; 4],
    border_styles: [u32; 4],
    effect_params: [f32; 4],
    clip_range: [u32; 2],
    gradient_color: [f32; 4],
    gradient: [f32; 4],
    transform_x: [f32; 3],
    transform_y: [f32; 3],
    image: [f32; 4],
}

impl ShapeInstance {
    fn new(
        rect: Rect,
        style: BoxStyle,
        clip_start: u32,
        clip_count: u32,
        effect_blur: f32,
        image_layer: Option<(u32, [f32; 2])>,
    ) -> Self {
        let border = style
            .border
            .filter(|border| border.widths().iter().any(|width| *width > 0.0));
        let outline = style.outline.filter(|outline| outline.width > 0.0);
        let (radii_x, radii_y) = style.corner_radius.normalized(rect);
        let border_colors = border.map_or([Color::TRANSPARENT; 4], Border::colors);
        let alpha = style.opacity.clamp(0.0, 1.0);
        let with_opacity = |mut color: [f32; 4]| {
            color[3] *= alpha;
            color
        };
        let (gradient_color, gradient) = match style.background_image.as_ref() {
            Some(BackgroundImage::LinearGradient {
                direction,
                start: _,
                end,
            }) => (
                with_opacity(end.components()),
                [direction[0], direction[1], 1.0, 0.0],
            ),
            Some(BackgroundImage::RadialGradient { start: _, end }) => {
                (with_opacity(end.components()), [0.0, 0.0, 2.0, 0.0])
            }
            Some(BackgroundImage::Raster(_)) => ([0.0; 4], [0.0, 0.0, 3.0, 0.0]),
            None => ([0.0; 4], [0.0; 4]),
        };
        let background = match style.background_image.as_ref() {
            Some(BackgroundImage::LinearGradient { start, .. })
            | Some(BackgroundImage::RadialGradient { start, .. }) => start,
            Some(BackgroundImage::Raster(_)) | None => &style.background,
        };
        let Transform { matrix } = style.transform;
        Self {
            center: [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5],
            half_size: [rect.width * 0.5, rect.height * 0.5],
            radii_x,
            radii_y,
            background: with_opacity(background.components()),
            border_colors: border_colors.map(|color| packed_color(color, alpha)),
            outline_color: with_opacity(
                outline
                    .map_or(Color::TRANSPARENT, |outline| outline.color)
                    .components(),
            ),
            border_widths: border.map_or([0.0; 4], |border| {
                let [top, right, bottom, left] = border.widths();
                [
                    top.clamp(0.0, rect.height),
                    right.clamp(0.0, rect.width),
                    bottom.clamp(0.0, rect.height),
                    left.clamp(0.0, rect.width),
                ]
            }),
            border_styles: border
                .map_or([0; 4], |border| border.styles().map(|style| style as u32)),
            effect_params: [
                outline.map_or(0.0, |outline| outline.width.max(0.0)),
                outline.map_or(0.0, |outline| outline.offset.max(0.0)),
                effect_blur.max(0.0),
                0.0,
            ],
            clip_range: [clip_start, clip_count],
            gradient_color,
            gradient,
            transform_x: [matrix[0], matrix[2], matrix[4]],
            transform_y: [matrix[1], matrix[3], matrix[5]],
            image: image_layer.map_or([0.0; 4], |(layer, scale)| {
                [scale[0], scale[1], layer as f32, alpha]
            }),
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 16] = wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x2,
            2 => Float32x4,
            3 => Float32x4,
            4 => Float32x4,
            5 => Uint32x4,
            6 => Float32x4,
            7 => Float32x4,
            8 => Uint32x4,
            9 => Float32x4,
            10 => Uint32x2,
            11 => Float32x4,
            12 => Float32x4,
            13 => Float32x3,
            14 => Float32x3,
            15 => Float32x4
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

fn packed_color(color: Color, opacity: f32) -> u32 {
    let [red, green, blue, alpha] = color.rgba8();
    let alpha = ((alpha as f32 * opacity.clamp(0.0, 1.0)).round() as u32).min(255);
    red as u32 | (green as u32) << 8 | (blue as u32) << 16 | alpha << 24
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct GpuClip {
    center: [f32; 2],
    half_size: [f32; 2],
    radii_x: [f32; 4],
    radii_y: [f32; 4],
}

impl GpuClip {
    fn new(clip: Clip) -> Self {
        let (radii_x, radii_y) = clip.corner_radius.normalized(clip.rect);
        Self {
            center: [
                clip.rect.x + clip.rect.width * 0.5,
                clip.rect.y + clip.rect.height * 0.5,
            ],
            half_size: [clip.rect.width * 0.5, clip.rect.height * 0.5],
            radii_x,
            radii_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Border, BorderSide, BorderStyle, CornerRadius, CornerSize};

    #[test]
    fn shape_instance_preserves_and_clamps_each_border_width() {
        let instance = ShapeInstance::new(
            Rect::new(0.0, 0.0, 30.0, 20.0),
            BoxStyle {
                border: Some(Border::per_side(-1.0, 40.0, 8.0, 12.0, Color::BLACK)),
                ..BoxStyle::default()
            },
            3,
            2,
            0.0,
            None,
        );

        assert_eq!(instance.border_widths, [0.0, 30.0, 8.0, 12.0]);
        assert_eq!(instance.clip_range, [3, 2]);
    }

    #[test]
    fn shape_instance_combines_per_side_borders_with_paint_effects() {
        let instance = ShapeInstance::new(
            Rect::new(0.0, 0.0, 80.0, 40.0),
            BoxStyle {
                background_image: Some(BackgroundImage::LinearGradient {
                    direction: [1.0, 0.0],
                    start: Color::WHITE,
                    end: Color::BLACK,
                }),
                corner_radius: CornerRadius::elliptical(
                    CornerSize::new(20.0, 8.0),
                    CornerSize::all(4.0),
                    CornerSize::ZERO,
                    CornerSize::ZERO,
                ),
                border: Some(Border::sides(
                    BorderSide::new(1.0, Color::from_rgba8(255, 0, 0, 255), BorderStyle::Solid),
                    BorderSide::new(2.0, Color::from_rgba8(0, 255, 0, 128), BorderStyle::Dashed),
                    BorderSide::new(3.0, Color::from_rgba8(0, 0, 255, 255), BorderStyle::Dotted),
                    BorderSide::new(4.0, Color::BLACK, BorderStyle::Double),
                )),
                opacity: 0.5,
                transform: Transform {
                    matrix: [1.0, 0.0, 0.0, 1.0, 5.0, 7.0],
                },
                ..BoxStyle::default()
            },
            0,
            0,
            0.0,
            None,
        );

        assert_eq!(instance.border_widths, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            instance.border_styles,
            [
                BorderStyle::Solid as u32,
                BorderStyle::Dashed as u32,
                BorderStyle::Dotted as u32,
                BorderStyle::Double as u32,
            ]
        );
        assert_eq!(instance.border_colors[0], 0x8000_00ff);
        assert_eq!(instance.border_colors[1], 0x4000_ff00);
        assert_eq!(instance.gradient, [1.0, 0.0, 1.0, 0.0]);
        assert_eq!(instance.transform_x, [1.0, 0.0, 5.0]);
        assert_eq!(instance.transform_y, [0.0, 1.0, 7.0]);
        assert_eq!(instance.radii_x[0], 20.0);
        assert_eq!(instance.radii_y[0], 8.0);
    }
}
