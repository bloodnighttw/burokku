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

use crate::ui::{UiDocument, UiLayout, UiUpdate};
use tokio::sync::mpsc::UnboundedReceiver;

pub fn run(
    document: UiDocument,
    updates: UnboundedReceiver<UiUpdate>,
) -> Result<(), Box<dyn Error>> {
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
    pending_document: UiDocument,
    updates: UnboundedReceiver<UiUpdate>,
    window: Option<Arc<Window>>,
    instance: Option<wgpu::Instance>,
    surface: Option<wgpu::Surface<'static>>,
    renderer: Option<Renderer>,
    text_system: TextSystem,
    layout: Option<UiLayout>,
    canvas: Canvas,
    pending_surface_size: Option<PhysicalSize<u32>>,
    canvas_dirty: bool,
    error: Option<String>,
}

impl UiApplication {
    fn new(document: UiDocument, updates: UnboundedReceiver<UiUpdate>) -> Self {
        Self {
            pending_document: document.clone(),
            document,
            updates,
            window: None,
            instance: None,
            surface: None,
            renderer: None,
            text_system: TextSystem::new(),
            layout: None,
            canvas: Canvas::new(),
            pending_surface_size: None,
            canvas_dirty: false,
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

        let scale_factor = window.scale_factor();
        self.relayout(size, scale_factor)?;
        self.repaint_canvas(scale_factor)?;
        self.renderer = Some(renderer);
        self.surface = Some(surface);
        self.instance = Some(instance);
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    fn relayout(
        &mut self,
        size: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<(), Box<dyn Error>> {
        let scale_factor = scale_factor as f32;
        if self.layout.is_none() {
            self.layout = Some(UiLayout::new(&self.document)?);
        }
        let layout = self.layout.as_mut().expect("layout was initialized");
        layout.relayout(
            size.width as f32 / scale_factor,
            size.height as f32 / scale_factor,
            &mut self.text_system,
        )?;
        Ok(())
    }

    fn repaint_canvas(&mut self, scale_factor: f64) -> Result<(), Box<dyn Error>> {
        let layout = self.layout.as_ref().expect("layout was initialized");
        self.canvas = layout.paint_with_scale(Color::WHITE, scale_factor as f32)?;
        Ok(())
    }

    fn queue_resize(&mut self, size: PhysicalSize<u32>) {
        self.pending_surface_size = Some(size);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn prepare_redraw(&mut self) -> Result<bool, Box<dyn Error>> {
        let pending_surface_size = self.pending_surface_size.take();
        let size =
            pending_surface_size.or_else(|| self.window.as_ref().map(|window| window.inner_size()));
        let Some(size) = size else {
            return Ok(false);
        };
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        let surface_size = SurfaceSize::new(size.width, size.height);
        let surface_size_changed = pending_surface_size.is_some()
            && self
                .renderer
                .as_ref()
                .is_some_and(|renderer| renderer.size() != surface_size);

        if pending_surface_size.is_some() || self.canvas_dirty {
            let scale_factor = self
                .window
                .as_ref()
                .map_or(1.0, |window| window.scale_factor());
            self.relayout(size, scale_factor)?;

            if surface_size_changed {
                if let (Some(renderer), Some(surface)) = (&mut self.renderer, &self.surface) {
                    renderer.resize(surface, surface_size);
                }
            }

            self.repaint_canvas(scale_factor)?;
            self.canvas_dirty = false;
        }

        Ok(true)
    }

    fn render(&mut self) -> Result<(), RenderError> {
        let (Some(renderer), Some(surface)) = (&mut self.renderer, &self.surface) else {
            return Ok(());
        };
        let window = self.window.as_ref().cloned();
        renderer.render_with_pre_present(
            surface,
            &self.canvas,
            &mut self.text_system,
            move || {
                if let Some(window) = window {
                    window.pre_present_notify();
                }
            },
        )?;
        Ok(())
    }

    fn apply_updates(&mut self) -> Result<bool, Box<dyn Error>> {
        let mut flushed = false;
        while let Ok(update) = self.updates.try_recv() {
            if self.pending_document.apply(update)? {
                self.document = self.pending_document.clone();
                flushed = true;
            }
        }
        if !flushed {
            return Ok(false);
        }
        self.layout = None;
        self.canvas_dirty = true;
        Ok(self.window.is_some())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl std::fmt::Display) {
        self.error = Some(error.to_string());
        event_loop.exit();
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        match self.prepare_redraw() {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        }

        match self.render() {
            Ok(()) | Err(RenderError::SurfaceTimeout | RenderError::SurfaceOccluded) => {}
            Err(RenderError::SurfaceLost | RenderError::SurfaceOutdated) => {
                let size = self
                    .window
                    .as_ref()
                    .expect("matching window exists")
                    .inner_size();
                self.queue_resize(size);
            }
            Err(error) => self.fail(event_loop, error),
        }
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
            WindowEvent::Resized(size) => self.queue_resize(size),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = self
                    .window
                    .as_ref()
                    .expect("matching window exists")
                    .inner_size();
                self.queue_resize(size);
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::Occluded(false) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
