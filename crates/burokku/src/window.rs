use std::{error::Error, sync::Arc, time::Duration};

use runtime::WindowEventMessage;
use tokio::sync::mpsc::Sender;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use crate::window::gpu::GPU;

mod gpu;

const FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub fn run(events: Sender<WindowEventMessage>) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let mut application = AppWindow::new(events);
    event_loop.run_app(&mut application)?;

    match application.error {
        Some(error) => Err(std::io::Error::other(error).into()),
        None => Ok(()),
    }
}

pub struct AppWindow {
    events: Sender<WindowEventMessage>,
    window: Option<Arc<Window>>,
    gpu: Option<GPU>,
    surface_version: u32,
    config_surface_version: u32,
    redraw_requested: bool,
    last_redraw: Option<std::time::Instant>,
    next_redraw_at: Option<std::time::Instant>,
    error: Option<String>,
}

impl AppWindow {
    fn new(events: Sender<WindowEventMessage>) -> Self {
        Self {
            events,
            window: None,
            gpu: None,
            surface_version: 0,
            config_surface_version: 0,
            redraw_requested: false,
            last_redraw: None,
            next_redraw_at: None,
            error: None,
        }
    }

    fn queue_surface(&mut self) {
        self.surface_version = self.surface_version.wrapping_add(1);
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl std::fmt::Display) {
        self.error = Some(error.to_string());
        event_loop.exit();
    }

    fn request_redraw(&mut self) {
        if self.redraw_requested {
            return;
        }

        if let Some(last_redraw) = self.last_redraw {
            let deadline = last_redraw + FRAME_INTERVAL;
            if std::time::Instant::now() < deadline {
                self.next_redraw_at = Some(deadline);
                return;
            }
        }

        if let Some(window) = &self.window {
            window.request_redraw();
            self.redraw_requested = true;
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        if self.window.is_some() {
            return Ok(());
        }

        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("Burokku")
                    .with_inner_size(LogicalSize::new(800.0, 600.0)),
            )?,
        );
        let gpu = GPU::new(window.clone())?;

        self.window = Some(window.clone());
        self.gpu = Some(gpu);
        self.request_redraw();
        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        self.redraw_requested = false;

        let (Some(window), Some(gpu)) = (self.window.as_ref().cloned(), self.gpu.as_mut()) else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        self.last_redraw = Some(std::time::Instant::now());

        if self.surface_version != self.config_surface_version {
            gpu.resize(size);
            self.config_surface_version = self.surface_version;
        }

        match gpu.render(&window) {
            Ok(())
            | Err(render::RenderError::SurfaceTimeout | render::RenderError::SurfaceOccluded) => {}
            Err(render::RenderError::SurfaceLost | render::RenderError::SurfaceOutdated) => {
                self.queue_surface();
                self.request_redraw();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }
}

impl ApplicationHandler for AppWindow {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.create_window(event_loop) {
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
            WindowEvent::CloseRequested => {
                let _ = self.events.try_send(WindowEventMessage::CloseRequested);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                let _ = self.events.try_send(WindowEventMessage::Resized {
                    width: size.width,
                    height: size.height,
                });
                self.queue_surface();
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = self.window.as_ref().map(|window| window.inner_size());
                if let Some(size) = size {
                    let _ = self.events.try_send(WindowEventMessage::Resized {
                        width: size.width,
                        height: size.height,
                    });
                }
                self.queue_surface();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::Occluded(false) => self.request_redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(deadline) = self.next_redraw_at {
            if std::time::Instant::now() >= deadline {
                self.next_redraw_at = None;
                self.request_redraw();
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                return;
            }
        }

        event_loop.set_control_flow(ControlFlow::Wait);
    }
}
