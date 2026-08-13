//! GPU pipeline for inward rounded-rectangle [`DrawCommand::Stroke`](crate::canvas::DrawCommand::Stroke) commands.

use bytemuck::{Pod, Zeroable};

use crate::{
    attributes::{corner::Corner, rect::Rect},
    canvas::DrawCommand,
    variants::clip::CLIP_FORMAT,
};

const INITIAL_INSTANCE_CAPACITY: u64 = std::mem::size_of::<StrokeInstance>() as u64;

/// Retained pipeline and upload buffers for stroke commands.
///
/// Each command maps to at most one instance, allowing the canvas to preserve
/// command order while switching between primitive pipelines.
pub(crate) struct StrokeRenderer {
    pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<StrokeInstance>,
    command_instances: Vec<Option<u32>>,
}

impl StrokeRenderer {
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render stroke screen uniform"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render stroke bind group layout"),
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
            label: Some("render stroke bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render stroke shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("wgsl/stroke.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render stroke pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render stroke pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[StrokeInstance::layout()],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: CLIP_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState {
                    front: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Equal,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Keep,
                    },
                    back: wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Equal,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Keep,
                    },
                    read_mask: u32::MAX,
                    write_mask: 0,
                },
                bias: wgpu::DepthBiasState::default(),
            }),
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
        let instance_buffer = create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY);

        Self {
            pipeline,
            screen_buffer,
            screen_bind_group,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            instances: Vec::new(),
            command_instances: Vec::new(),
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

        self.collect_instances(commands);
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

    pub(crate) fn draw_command<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        command_index: usize,
    ) {
        let Some(instance) = self.command_instances[command_index] else {
            return;
        };

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.draw(0..6, instance..instance + 1);
    }

    fn collect_instances(&mut self, commands: &[DrawCommand]) {
        self.instances.clear();
        self.command_instances.clear();
        self.command_instances.resize(commands.len(), None);

        for (command_index, command) in commands.iter().enumerate() {
            let DrawCommand::Stroke {
                rect,
                width,
                corners,
                color,
            } = command
            else {
                continue;
            };
            if !is_valid_rect(*rect) || !width.is_finite() || *width <= 0.0 {
                continue;
            }

            let instance = self.instances.len() as u32;
            self.instances
                .push(StrokeInstance::new(*rect, *width, *corners, *color));
            self.command_instances[command_index] = Some(instance);
        }
    }
}

fn create_instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render stroke instance buffer"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn is_valid_rect(rect: Rect) -> bool {
    rect.has_area()
        && rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct ScreenUniform {
    size: [f32; 2],
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct StrokeInstance {
    bounds: [f32; 4],
    corners: [f32; 4],
    color: [f32; 4],
    stroke: [f32; 4],
}

impl StrokeInstance {
    fn new(rect: Rect, width: f32, corners: Corner, color: wgpu::Color) -> Self {
        Self {
            bounds: [rect.x, rect.y, rect.width, rect.height],
            corners: fit_corners(corners, rect.width, rect.height),
            color: [
                color.r as f32,
                color.g as f32,
                color.b as f32,
                color.a as f32,
            ],
            stroke: [width, 0.0, 0.0, 0.0],
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Float32x4,
            3 => Float32x4
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRIBUTES,
        }
    }
}

fn fit_corners(corners: Corner, width: f32, height: f32) -> [f32; 4] {
    let mut radii = [corners.lt, corners.rt, corners.br, corners.bl].map(|radius| {
        if radius.is_finite() {
            radius.max(0.0)
        } else {
            0.0
        }
    });
    let scale = [
        edge_scale(width, radii[0] + radii[1]),
        edge_scale(height, radii[1] + radii[2]),
        edge_scale(width, radii[2] + radii[3]),
        edge_scale(height, radii[3] + radii[0]),
    ]
    .into_iter()
    .fold(1.0_f32, f32::min);
    radii.iter_mut().for_each(|radius| *radius *= scale);
    radii
}

fn edge_scale(length: f32, radii: f32) -> f32 {
    if radii > length {
        length / radii
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::mpsc;

    #[cfg(not(target_arch = "wasm32"))]
    use crate::{canvas::OffscreenCanvas, engine::Engine};

    use super::*;

    #[test]
    fn gpu_types_have_wgsl_compatible_layouts() {
        assert_eq!(std::mem::size_of::<ScreenUniform>(), 16);
        assert_eq!(std::mem::size_of::<StrokeInstance>(), 64);
    }

    #[test]
    fn instance_converts_and_fits_stroke_values() {
        let instance = StrokeInstance::new(
            Rect::new(1.0, 2.0, 40.0, 20.0),
            3.0,
            Corner::new(30.0, 30.0, f32::NAN, -4.0),
            wgpu::Color {
                r: 0.25,
                g: 0.5,
                b: 0.75,
                a: 1.0,
            },
        );

        assert_eq!(instance.bounds, [1.0, 2.0, 40.0, 20.0]);
        assert_eq!(instance.corners, [20.0, 20.0, 0.0, 0.0]);
        assert_eq!(instance.color, [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(instance.stroke, [3.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn shader_is_valid_wgsl() {
        naga::front::wgsl::parse_str(include_str!("wgsl/stroke.wgsl"))
            .expect("stroke shader should parse");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_canvas_renders_inward_rounded_strokes() {
        let Some(mut canvas) = create_offscreen_canvas([20, 16]).await else {
            return;
        };

        canvas
            .draw(
                &[
                    DrawCommand::Stroke {
                        rect: Rect::new(2.0, 2.0, 10.0, 12.0),
                        width: 2.0,
                        corners: Corner::new(4.0, 4.0, 4.0, 4.0),
                        color: wgpu::Color::RED,
                    },
                    DrawCommand::Stroke {
                        rect: Rect::new(14.0, 2.0, 4.0, 4.0),
                        width: 3.0,
                        corners: Corner::default(),
                        color: wgpu::Color::GREEN,
                    },
                ],
                wgpu::Color::BLUE,
            )
            .unwrap();

        let pixels = read_offscreen_pixels(&canvas);
        for (name, x, y, expected) in [
            ("outside", 1, 8, [0, 0, 255, 255]),
            ("rounded outer corner", 2, 2, [0, 0, 255, 255]),
            ("top edge", 7, 2, [255, 0, 0, 255]),
            ("left edge", 2, 8, [255, 0, 0, 255]),
            ("hollow center", 7, 8, [0, 0, 255, 255]),
            ("thick stroke fills bounds", 15, 3, [0, 255, 0, 255]),
        ] {
            assert_eq!(
                pixel(&pixels, canvas.size(), x, y),
                expected,
                "{name} pixel has the wrong color",
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn create_offscreen_canvas(size: [u32; 2]) -> Option<OffscreenCanvas> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        {
            Ok(adapter) => adapter,
            Err(error) => {
                eprintln!("skipping offscreen stroke test: no WebGPU adapter available: {error}");
                return None;
            }
        };
        let Ok((device, queue)) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("render stroke test device"),
                ..Default::default()
            })
            .await
        else {
            eprintln!("skipping offscreen stroke test: WebGPU device creation failed");
            return None;
        };
        OffscreenCanvas::new(Engine::new(&device, &queue), size[0], size[1]).ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_offscreen_pixels(canvas: &OffscreenCanvas) -> Vec<u8> {
        let [width, height] = canvas.size();
        let unpadded_bytes_per_row = width * 4;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let engine = canvas.engine();
        let readback = engine.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("render stroke test readback"),
            size: padded_bytes_per_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            engine.create_command_encoder(Some("render stroke test readback encoder"));
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: canvas.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        engine.submit(encoder.finish());

        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        engine
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receiver.recv().unwrap().unwrap();

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
        for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
            pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
        }
        drop(mapped);
        readback.unmap();
        pixels
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pixel(pixels: &[u8], size: [u32; 2], x: u32, y: u32) -> [u8; 4] {
        let start = ((y * size[0] + x) * 4) as usize;
        pixels[start..start + 4].try_into().unwrap()
    }
}
