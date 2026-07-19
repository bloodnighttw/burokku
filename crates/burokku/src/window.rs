use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use render::{wgpu, Canvas, Color, RenderError, Renderer, SurfaceSize, TextSystem};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use crate::ui::{UiDocument, UiLayout};
use tokio::sync::mpsc::UnboundedReceiver;

pub fn run(document: UiDocument, updates: UnboundedReceiver<String>) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let mut application = UiApplication::new(document, updates);
    event_loop.run_app(&mut application)?;
    if let Some(error) = application.error {
        return Err(std::io::Error::other(error).into());
    }
    Ok(())
}

struct UiApplication {
    document: UiDocument,
    updates: UnboundedReceiver<String>,
    window: Option<Arc<Window>>,
    instance: Option<wgpu::Instance>,
    surface: Option<wgpu::Surface<'static>>,
    renderer: Option<Renderer>,
    text_system: TextSystem,
    canvas: Canvas,
    error: Option<String>,
}

impl UiApplication {
    fn new(document: UiDocument, updates: UnboundedReceiver<String>) -> Self {
        Self {
            document,
            updates,
            window: None,
            instance: None,
            surface: None,
            renderer: None,
            text_system: TextSystem::new(),
            canvas: Canvas::new(),
            error: None,
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        if self.window.is_some() {
            return Ok(());
        }
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Burokku React Example")
                    .with_inner_size(LogicalSize::new(800.0, 600.0)),
            )?,
        );
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface: wgpu::Surface<'static> = instance.create_surface(window.clone())?;
        let renderer = pollster::block_on(Renderer::new(
            &instance,
            &surface,
            SurfaceSize::new(size.width, size.height),
        ))?;

        self.rebuild_canvas(size, window.scale_factor())?;
        self.renderer = Some(renderer);
        self.surface = Some(surface);
        self.instance = Some(instance);
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    fn rebuild_canvas(
        &mut self,
        size: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<(), Box<dyn Error>> {
        let scale_factor = scale_factor as f32;
        let layout = UiLayout::compute(
            &self.document,
            size.width as f32 / scale_factor,
            size.height as f32 / scale_factor,
            &mut self.text_system,
        )?;
        self.canvas = layout.paint_with_scale(Color::WHITE, scale_factor)?;
        Ok(())
    }

    fn resize(&mut self, size: PhysicalSize<u32>) -> Result<(), Box<dyn Error>> {
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if let (Some(renderer), Some(surface)) = (&mut self.renderer, &self.surface) {
            renderer.resize(surface, SurfaceSize::new(size.width, size.height));
        }
        let scale_factor = self
            .window
            .as_ref()
            .map_or(1.0, |window| window.scale_factor());
        self.rebuild_canvas(size, scale_factor)?;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        Ok(())
    }

    fn render(&mut self) -> Result<(), RenderError> {
        let (Some(renderer), Some(surface)) = (&mut self.renderer, &self.surface) else {
            return Ok(());
        };
        renderer.render(surface, &self.canvas, &mut self.text_system)
    }

    fn apply_updates(&mut self) -> Result<bool, Box<dyn Error>> {
        let mut latest = None;
        while let Ok(snapshot) = self.updates.try_recv() {
            latest = Some(snapshot);
        }
        let Some(snapshot) = latest else {
            return Ok(false);
        };
        self.document = UiDocument::from_json(&snapshot)?;
        let Some(window) = &self.window else {
            return Ok(false);
        };
        self.rebuild_canvas(window.inner_size(), window.scale_factor())?;
        Ok(true)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl std::fmt::Display) {
        self.error = Some(error.to_string());
        event_loop.exit();
    }
}

impl ApplicationHandler for UiApplication {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
        match self.apply_updates() {
            Ok(true) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Ok(false) => {}
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.initialize(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Err(error) = self.resize(size) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = self
                    .window
                    .as_ref()
                    .expect("matching window exists")
                    .inner_size();
                if let Err(error) = self.resize(size) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::RedrawRequested => match self.render() {
                Ok(()) | Err(RenderError::SurfaceTimeout | RenderError::SurfaceOccluded) => {}
                Err(RenderError::SurfaceLost | RenderError::SurfaceOutdated) => {
                    let size = self
                        .window
                        .as_ref()
                        .expect("matching window exists")
                        .inner_size();
                    if let Err(error) = self.resize(size) {
                        self.fail(event_loop, error);
                    }
                }
                Err(error) => self.fail(event_loop, error),
            },
            WindowEvent::Occluded(false) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
