use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use liquid_glass::LiquidGlassRenderer;
use vello::kurbo::{Affine, Circle, Rect};
use vello::peniko::{Color, Fill, color::palette};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::{self, CurrentSurfaceTexture};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

mod liquid_glass;

enum RenderState {
    Active {
        surface: Box<RenderSurface<'static>>,
        valid_surface: bool,
        window: Arc<Window>,
    },
    Suspended(Option<Arc<Window>>),
}

struct App {
    context: RenderContext,
    renderers: Vec<Option<Renderer>>,
    glass_renderers: Vec<Option<LiquidGlassRenderer>>,
    state: RenderState,
    scene: Scene,
    started_at: Instant,
    frame_index: u64,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let RenderState::Suspended(cached_window) = &mut self.state else {
            return;
        };

        let window = cached_window
            .take()
            .unwrap_or_else(|| create_window(event_loop));
        let size = window.inner_size();
        let surface = pollster::block_on(self.context.create_surface(
            window.clone(),
            size.width,
            size.height,
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("failed to create a Vello render surface");
        let device_id = surface.dev_id;

        self.renderers
            .resize_with(self.context.devices.len(), || None);
        self.renderers[device_id].get_or_insert_with(|| create_renderer(&self.context, &surface));

        self.glass_renderers
            .resize_with(self.context.devices.len(), || None);
        let device = &self.context.devices[device_id].device;
        match &mut self.glass_renderers[device_id] {
            Some(glass) => glass.set_background(device, &surface.target_view),
            slot @ None => {
                *slot = Some(LiquidGlassRenderer::new(
                    device,
                    surface.format,
                    &surface.target_view,
                ));
            }
        }

        window.request_redraw();
        self.state = RenderState::Active {
            surface: Box::new(surface),
            valid_surface: true,
            window,
        };
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let RenderState::Active { window, .. } = &self.state {
            self.state = RenderState::Suspended(Some(window.clone()));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let RenderState::Active {
            surface,
            valid_surface,
            window,
        } = &mut self.state
        else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    *valid_surface = false;
                } else {
                    self.context
                        .resize_surface(surface, size.width, size.height);
                    let device = &self.context.devices[surface.dev_id].device;
                    self.glass_renderers[surface.dev_id]
                        .as_mut()
                        .expect("glass renderer must exist for the active surface")
                        .set_background(device, &surface.target_view);
                    *valid_surface = true;
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested if *valid_surface => {
                let draw_started = Instant::now();
                let elapsed_seconds = self.started_at.elapsed().as_secs_f32();
                let width = surface.config.width;
                let height = surface.config.height;
                let device_handle = &self.context.devices[surface.dev_id];

                self.scene.reset();
                add_backdrop_to_scene(&mut self.scene, width, height, elapsed_seconds);
                self.renderers[surface.dev_id]
                    .as_mut()
                    .expect("renderer must exist for the active surface")
                    .render_to_texture(
                        &device_handle.device,
                        &device_handle.queue,
                        &self.scene,
                        &surface.target_view,
                        &RenderParams {
                            base_color: palette::css::BLACK,
                            width,
                            height,
                            antialiasing_method: AaConfig::Msaa16,
                        },
                    )
                    .expect("failed to render the Vello backdrop");

                let surface_texture = match surface.surface.get_current_texture() {
                    CurrentSurfaceTexture::Success(texture) => texture,
                    CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Suboptimal(_) => {
                        self.context.configure_surface(surface);
                        window.request_redraw();
                        return;
                    }
                    CurrentSurfaceTexture::Occluded | CurrentSurfaceTexture::Timeout => {
                        window.request_redraw();
                        return;
                    }
                    CurrentSurfaceTexture::Lost => panic!("render surface was lost"),
                    CurrentSurfaceTexture::Validation => {
                        panic!("validation error while acquiring the render surface")
                    }
                };

                let mut encoder =
                    device_handle
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Vello and liquid-glass frame"),
                        });
                let surface_view = surface_texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                // Copy the Vello backdrop to the window, then blend every glass pane
                // in one instanced render pass while sampling the Vello texture.
                surface.blitter.copy(
                    &device_handle.device,
                    &mut encoder,
                    &surface.target_view,
                    &surface_view,
                );
                let glass = self.glass_renderers[surface.dev_id]
                    .as_ref()
                    .expect("glass renderer must exist for the active surface");
                glass.draw(
                    &device_handle.queue,
                    &mut encoder,
                    &surface_view,
                    width,
                    height,
                    elapsed_seconds,
                );

                device_handle.queue.submit([encoder.finish()]);
                surface_texture.present();
                device_handle
                    .device
                    .poll(wgpu::PollType::Poll)
                    .expect("failed to poll the GPU device");

                self.frame_index += 1;
                eprintln!(
                    "frame {:06} | draw {:8.3} ms CPU | {} liquid-glass panes",
                    self.frame_index,
                    draw_started.elapsed().as_secs_f64() * 1_000.0,
                    glass.instance_count(),
                );
                window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    let mut app = App {
        context: RenderContext::new(),
        renderers: Vec::new(),
        glass_renderers: Vec::new(),
        state: RenderState::Suspended(None),
        scene: Scene::new(),
        started_at: Instant::now(),
        frame_index: 0,
    };

    EventLoop::new()?.run_app(&mut app)?;
    Ok(())
}

fn create_window(event_loop: &ActiveEventLoop) -> Arc<Window> {
    let attributes = Window::default_attributes()
        .with_title("Vello + WGSL: configurable liquid-glass stress test")
        .with_inner_size(LogicalSize::new(1000, 700))
        .with_resizable(true);
    Arc::new(
        event_loop
            .create_window(attributes)
            .expect("failed to create a window"),
    )
}

fn create_renderer(context: &RenderContext, surface: &RenderSurface<'_>) -> Renderer {
    Renderer::new(
        &context.devices[surface.dev_id].device,
        RendererOptions::default(),
    )
    .expect("failed to create a Vello renderer")
}

fn add_backdrop_to_scene(scene: &mut Scene, width: u32, height: u32, time: f32) {
    let width = width as f64;
    let height = height as f64;
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(16, 21, 42),
        None,
        &Rect::new(0.0, 0.0, width, height),
    );

    let palette = [
        Color::from_rgb8(255, 91, 132),
        Color::from_rgb8(106, 167, 255),
        Color::from_rgb8(91, 230, 183),
        Color::from_rgb8(255, 190, 92),
        Color::from_rgb8(183, 112, 255),
    ];

    for index in 0..28 {
        let phase = index as f64 * 1.73;
        let x =
            ((phase * 91.0 + time as f64 * (12.0 + index as f64 % 5.0)) % (width + 240.0)) - 120.0;
        let y = 40.0 + (index as f64 * 83.0) % height.max(80.0);
        let radius = 42.0 + (index % 6) as f64 * 13.0;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            palette[index % palette.len()],
            None,
            &Circle::new((x, y), radius),
        );
    }

    for index in 0..14 {
        let x = index as f64 * width / 13.0 - 18.0;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(255, 255, 255, 34),
            None,
            &Rect::new(x, 0.0, x + 3.0, height),
        );
    }
}
