use std::sync::Arc;

use anyhow::Result;
use vello::kurbo::{Affine, BezPath, Circle, RoundedRect, Stroke};
use vello::peniko::{Color, Fill, color::palette};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu::{self, CurrentSurfaceTexture};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

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
    state: RenderState,
    scene: Scene,
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

        self.renderers
            .resize_with(self.context.devices.len(), || None);
        self.renderers[surface.dev_id]
            .get_or_insert_with(|| create_renderer(&self.context, &surface));

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
                    *valid_surface = true;
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested if *valid_surface => {
                self.scene.reset();
                add_shapes_to_scene(&mut self.scene);

                let width = surface.config.width;
                let height = surface.config.height;
                let device_handle = &self.context.devices[surface.dev_id];

                self.renderers[surface.dev_id]
                    .as_mut()
                    .expect("renderer must exist for the active surface")
                    .render_to_texture(
                        &device_handle.device,
                        &device_handle.queue,
                        &self.scene,
                        &surface.target_view,
                        &RenderParams {
                            base_color: palette::css::WHITE,
                            width,
                            height,
                            antialiasing_method: AaConfig::Msaa16,
                        },
                    )
                    .expect("failed to render the Vello scene");

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
                            label: Some("Vello surface blit"),
                        });
                let surface_view = surface_texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                surface.blitter.copy(
                    &device_handle.device,
                    &mut encoder,
                    &surface.target_view,
                    &surface_view,
                );
                device_handle.queue.submit([encoder.finish()]);
                surface_texture.present();
                device_handle
                    .device
                    .poll(wgpu::PollType::Poll)
                    .expect("failed to poll the GPU device");
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    let mut app = App {
        context: RenderContext::new(),
        renderers: Vec::new(),
        state: RenderState::Suspended(None),
        scene: Scene::new(),
    };

    EventLoop::new()?.run_app(&mut app)?;
    Ok(())
}

fn create_window(event_loop: &ActiveEventLoop) -> Arc<Window> {
    let attributes = Window::default_attributes()
        .with_title("Vello shape example")
        .with_inner_size(LogicalSize::new(800, 600))
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

fn add_shapes_to_scene(scene: &mut Scene) {
    // A filled circle.
    let circle = Circle::new((185.0, 190.0), 105.0);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(242, 140, 168),
        None,
        &circle,
    );

    // A rounded rectangle with a thick outline.
    let card = RoundedRect::new(350.0, 85.0, 690.0, 295.0, 28.0);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(232, 239, 255),
        None,
        &card,
    );
    scene.stroke(
        &Stroke::new(8.0),
        Affine::IDENTITY,
        Color::from_rgb8(52, 82, 163),
        None,
        &card,
    );

    // A custom triangle path.
    let mut triangle = BezPath::new();
    triangle.move_to((400.0, 500.0));
    triangle.line_to((560.0, 340.0));
    triangle.line_to((720.0, 500.0));
    triangle.close_path();
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(75, 181, 125),
        None,
        &triangle,
    );
}
