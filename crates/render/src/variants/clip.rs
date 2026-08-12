//! Stencil pipeline for nested rectangular and rounded clip commands.

use bytemuck::{Pod, Zeroable};

use crate::{
    attributes::{corner::Corner, rect::Rect},
    canvas::DrawCommand,
};

pub(crate) const CLIP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
pub(crate) const MAX_CLIP_DEPTH: usize = u8::MAX as usize;

const INITIAL_INSTANCE_CAPACITY: u64 = std::mem::size_of::<ClipInstance>() as u64;

/// Retained pipelines and instances used to mutate the canvas stencil stack.
pub(crate) struct ClipRenderer {
    push_pipeline: wgpu::RenderPipeline,
    pop_pipeline: wgpu::RenderPipeline,
    screen_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instances: Vec<ClipInstance>,
    command_instances: Vec<Option<u32>>,
}

impl ClipRenderer {
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("render clip screen uniform"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render clip bind group layout"),
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
            label: Some("render clip bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render clip shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("wgsl/clip.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("render clip pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let push_pipeline = create_pipeline(
            device,
            &shader,
            &pipeline_layout,
            target_format,
            wgpu::StencilOperation::IncrementClamp,
            "render push clip pipeline",
        );
        let pop_pipeline = create_pipeline(
            device,
            &shader,
            &pipeline_layout,
            target_format,
            wgpu::StencilOperation::DecrementClamp,
            "render pop clip pipeline",
        );
        let instance_buffer = create_instance_buffer(device, INITIAL_INSTANCE_CAPACITY);

        Self {
            push_pipeline,
            pop_pipeline,
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

    pub(crate) fn push<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        command_index: usize,
        current_depth: u32,
    ) {
        self.draw_command(pass, command_index, current_depth, &self.push_pipeline);
    }

    pub(crate) fn pop<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        command_index: usize,
        current_depth: u32,
    ) {
        self.draw_command(pass, command_index, current_depth, &self.pop_pipeline);
    }

    fn draw_command<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        command_index: usize,
        stencil_reference: u32,
        pipeline: &'pass wgpu::RenderPipeline,
    ) {
        let instance = self.command_instances[command_index]
            .expect("balanced clip commands should have a prepared instance");
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
        pass.set_stencil_reference(stencil_reference);
        pass.draw(0..6, instance..instance + 1);
    }

    fn collect_instances(&mut self, commands: &[DrawCommand]) {
        self.instances.clear();
        self.command_instances.clear();
        self.command_instances.resize(commands.len(), None);
        let mut stack = Vec::new();

        for (command_index, command) in commands.iter().enumerate() {
            match command {
                DrawCommand::PushClip { rect, corners } => {
                    let instance = self.instances.len() as u32;
                    self.instances.push(ClipInstance::new(*rect, *corners));
                    self.command_instances[command_index] = Some(instance);
                    stack.push(instance);
                }
                DrawCommand::PopClip => {
                    self.command_instances[command_index] = stack.pop();
                }
                DrawCommand::Fill { .. } | DrawCommand::Stroke { .. } => {}
            }
        }
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    pass_op: wgpu::StencilOperation,
    label: &'static str,
) -> wgpu::RenderPipeline {
    let stencil_face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[ClipInstance::layout()],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: CLIP_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: stencil_face,
                back: stencil_face,
                read_mask: u32::MAX,
                write_mask: u32::MAX,
            },
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("render clip instance buffer"),
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
struct ClipInstance {
    bounds: [f32; 4],
    corners: [f32; 4],
}

impl ClipInstance {
    fn new(rect: Rect, corners: Corner) -> Self {
        let bounds = if rect.has_area()
            && rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
        {
            [rect.x, rect.y, rect.width, rect.height]
        } else {
            [0.0; 4]
        };
        Self {
            bounds,
            corners: fit_corners(corners, bounds[2], bounds[3]),
        }
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
    if length.is_finite() && length > 0.0 && radii > length {
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
        assert_eq!(std::mem::size_of::<ClipInstance>(), 32);
    }

    #[test]
    fn oversized_and_invalid_corners_are_fitted() {
        let instance = ClipInstance::new(
            Rect::new(0.0, 0.0, 40.0, 20.0),
            Corner::new(30.0, 30.0, f32::NAN, -4.0),
        );
        assert_eq!(instance.corners, [20.0, 20.0, 0.0, 0.0]);
    }

    #[test]
    fn invalid_rectangles_become_empty_clip_masks() {
        assert_eq!(
            ClipInstance::new(
                Rect::new(2.0, 2.0, -4.0, 8.0),
                Corner::new(2.0, 2.0, 2.0, 2.0)
            )
            .bounds,
            [0.0; 4]
        );
        assert_eq!(
            ClipInstance::new(Rect::new(f32::NAN, 2.0, 4.0, 8.0), Corner::default()).bounds,
            [0.0; 4]
        );
    }

    #[test]
    fn shader_is_valid_wgsl() {
        naga::front::wgsl::parse_str(include_str!("wgsl/clip.wgsl"))
            .expect("clip shader should parse");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_canvas_applies_and_restores_nested_clips() {
        let Some(mut canvas) = create_offscreen_canvas([16, 16]).await else {
            return;
        };
        let round = Corner::new(4.0, 4.0, 4.0, 4.0);

        canvas
            .draw(
                &[
                    fill(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED),
                    DrawCommand::PushClip {
                        rect: Rect::new(2.0, 2.0, 12.0, 12.0),
                        corners: round,
                    },
                    fill(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::GREEN),
                    DrawCommand::PushClip {
                        rect: Rect::new(6.0, 0.0, 4.0, 16.0),
                        corners: Corner::default(),
                    },
                    fill(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::WHITE),
                    DrawCommand::PopClip,
                    fill(Rect::new(3.0, 7.0, 2.0, 2.0), wgpu::Color::BLACK),
                    DrawCommand::PopClip,
                    DrawCommand::PushClip {
                        rect: Rect::new(8.0, 8.0, 0.0, 4.0),
                        corners: Corner::default(),
                    },
                    fill(
                        Rect::new(0.0, 0.0, 16.0, 16.0),
                        wgpu::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 0.0,
                            a: 1.0,
                        },
                    ),
                    DrawCommand::PopClip,
                    fill(Rect::new(0.0, 0.0, 1.0, 1.0), wgpu::Color::BLUE),
                    fill(Rect::new(15.0, 15.0, 1.0, 1.0), wgpu::Color::BLUE),
                ],
                wgpu::Color::BLACK,
            )
            .unwrap();

        let pixels = read_offscreen_pixels(&canvas);
        assert_eq!(pixel(&pixels, canvas.size(), 0, 0), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 1, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 2, 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 3, 3), [0, 255, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 6, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 6, 2), [255, 255, 255, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 9, 13), [255, 255, 255, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 10, 8), [0, 255, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 3, 7), [0, 0, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 13, 13), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, canvas.size(), 15, 15), [0, 0, 255, 255]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_canvas_clips_all_four_large_rounded_corners() {
        let Some(mut canvas) = create_offscreen_canvas([32, 32]).await else {
            return;
        };
        let clip = Rect::new(4.0, 4.0, 24.0, 24.0);
        let large_round = Corner::new(10.0, 10.0, 10.0, 10.0);

        canvas
            .draw(
                &[
                    DrawCommand::PushClip {
                        rect: clip,
                        corners: large_round,
                    },
                    fill(Rect::new(0.0, 0.0, 32.0, 32.0), wgpu::Color::RED),
                    DrawCommand::PopClip,
                ],
                wgpu::Color::BLUE,
            )
            .unwrap();

        let pixels = read_offscreen_pixels(&canvas);
        for (name, x, y) in [
            ("left-top", 4, 4),
            ("right-top", 27, 4),
            ("right-bottom", 27, 27),
            ("left-bottom", 4, 27),
        ] {
            assert_eq!(
                pixel(&pixels, canvas.size(), x, y),
                [0, 0, 255, 255],
                "{name} rounded corner must retain the background color",
            );
        }

        for (name, x, y) in [
            ("top edge", 16, 4),
            ("right edge", 27, 16),
            ("bottom edge", 16, 27),
            ("left edge", 4, 16),
            ("center", 16, 16),
        ] {
            assert_eq!(
                pixel(&pixels, canvas.size(), x, y),
                [255, 0, 0, 255],
                "{name} must contain the fill color",
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
                eprintln!("skipping offscreen clip test: no WebGPU adapter available: {error}");
                return None;
            }
        };
        let Ok((device, queue)) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("render clip test device"),
                ..Default::default()
            })
            .await
        else {
            eprintln!("skipping offscreen clip test: WebGPU device creation failed");
            return None;
        };
        OffscreenCanvas::new(Engine::new(&device, &queue), size[0], size[1]).ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn fill(rect: Rect, color: wgpu::Color) -> DrawCommand {
        DrawCommand::Fill { rect, color }
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
            label: Some("render clip test readback"),
            size: padded_bytes_per_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = engine.create_command_encoder(Some("render clip test readback encoder"));
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
