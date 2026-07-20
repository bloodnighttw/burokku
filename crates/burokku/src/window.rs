use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use runtime::{InputState, ModifiersState, MouseButton, WindowEventMessage};
use tokio::sync::mpsc::Sender;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, Modifiers, MouseButton as NativeMouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use crate::dom::DomStore;
use crate::window::gpu::GPU;

mod gpu;

pub async fn run(events: Sender<WindowEventMessage>, dom: DomStore) -> Result<(), Box<dyn Error>> {
    let mut event_loop = EventLoop::new()?;
    let window = Arc::new(
        event_loop.create_window(
            Window::default_attributes()
                .with_title("Burokku")
                .with_inner_size(LogicalSize::new(800.0, 600.0)),
        )?,
    );
    let gpu = GPU::new(window.clone(), dom).await?;
    let application = AppWindow::new(events, window, gpu);
    let application = event_loop.run_app(application).await?;

    match application.error {
        Some(error) => Err(std::io::Error::other(error).into()),
        None => Ok(()),
    }
}

pub struct AppWindow {
    events: Sender<WindowEventMessage>,
    window: Arc<Window>,
    gpu: GPU,
    surface_version: u16,
    config_surface_version: u16,
    error: Option<String>,
}

impl AppWindow {
    fn new(events: Sender<WindowEventMessage>, window: Arc<Window>, gpu: GPU) -> Self {
        Self {
            events,
            window,
            gpu,
            surface_version: 0,
            config_surface_version: 0,
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
        self.window.request_redraw();
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let window = self.window.clone();
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        if self.surface_version != self.config_surface_version {
            if let Err(error) = self.gpu.resize(size, window.scale_factor()) {
                self.fail(event_loop, error);
                return;
            }
            self.config_surface_version = self.surface_version;
        }

        match self.gpu.render(&window) {
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
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.id() != window_id {
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
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => {
                let _ = self
                    .events
                    .try_send(WindowEventMessage::ScaleFactorChanged {
                        scale_factor,
                        width: new_inner_size.width,
                        height: new_inner_size.height,
                    });
                self.queue_surface();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::Focused(focused) => {
                let _ = self.events.try_send(WindowEventMessage::Focused(focused));
            }
            WindowEvent::Occluded(occluded) => {
                let _ = self.events.try_send(WindowEventMessage::Occluded(occluded));
                if !occluded {
                    self.request_redraw();
                }
            }
            WindowEvent::KeyboardInput(event) => {
                let _ = self.events.try_send(WindowEventMessage::KeyboardInput {
                    key_code: event.key_code,
                    text: event.text,
                    state: input_state(event.state),
                    repeat: event.repeat,
                    modifiers: modifiers(event.modifiers),
                });
            }
            WindowEvent::ModifiersChanged(state) => {
                let _ = self
                    .events
                    .try_send(WindowEventMessage::ModifiersChanged(modifiers(state)));
            }
            WindowEvent::CursorMoved { position } => {
                let _ = self.events.try_send(WindowEventMessage::CursorMoved {
                    x: position.x,
                    y: position.y,
                });
            }
            WindowEvent::MouseInput { state, button } => {
                let _ = self.events.try_send(WindowEventMessage::MouseInput {
                    state: input_state(state),
                    button: mouse_button(button),
                });
            }
            WindowEvent::MouseWheel {
                delta_x,
                delta_y,
                precise,
            } => {
                let _ = self.events.try_send(WindowEventMessage::MouseWheel {
                    delta_x,
                    delta_y,
                    precise,
                });
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.gpu.sync_dom(&self.window) {
            Ok(true) => self.request_redraw(),
            Ok(false) => {}
            Err(error) => self.fail(event_loop, error),
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

fn input_state(state: ElementState) -> InputState {
    match state {
        ElementState::Pressed => InputState::Pressed,
        ElementState::Released => InputState::Released,
    }
}

fn mouse_button(button: NativeMouseButton) -> MouseButton {
    match button {
        NativeMouseButton::Left => MouseButton::Left,
        NativeMouseButton::Right => MouseButton::Right,
        NativeMouseButton::Middle => MouseButton::Middle,
        NativeMouseButton::Other(button) => MouseButton::Other(button),
    }
}

fn modifiers(modifiers: Modifiers) -> ModifiersState {
    ModifiersState {
        shift: modifiers.shift,
        control: modifiers.control,
        alt: modifiers.alt,
        command: modifiers.command,
        caps_lock: modifiers.caps_lock,
    }
}
