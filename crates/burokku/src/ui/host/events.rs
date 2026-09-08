//! Native event translation and `winit` callback dispatch.

use std::rc::Rc;

use winit::{
    application::ApplicationHandler, ActiveEventLoop, ElementState, KeyEvent, Modifiers,
    MouseButton, PhysicalPosition, WindowEvent, WindowId,
};

use crate::app::RuntimeStatus;

use super::{
    super::{elements::NodeId, gpu::PresentationOutcome, scene::ScenePlan},
    render::{classify_resize_failure, presentation_state, PresentedSurface},
    ApplicationHost, HostError,
};

/// DOM `MouseEvent.button`: the button changed by this event.
/// `None` means no button changed and is exposed to JavaScript as `-1`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ChangedMouseButton(Option<u16>);

impl ChangedMouseButton {
    pub(crate) const NONE: Self = Self(None);
    pub(crate) const PRIMARY: Self = Self(Some(0));
    pub(crate) const AUXILIARY: Self = Self(Some(1));
    pub(crate) const SECONDARY: Self = Self(Some(2));

    pub(crate) const fn from_code(code: u16) -> Self {
        Self(Some(code))
    }

    pub(crate) fn code(self) -> i32 {
        self.0.map_or(-1, i32::from)
    }

    pub(crate) fn buttons_bit(self) -> u16 {
        match self.0 {
            None => 0,
            Some(0) => 1,
            Some(1) => 4,
            Some(2) => 2,
            Some(code) => 1_u16.checked_shl(code as u32).unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PressedMouseButtons(u16);

impl PressedMouseButtons {
    pub(crate) const NONE: Self = Self(0);

    pub(crate) const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    pub(crate) const fn bits(self) -> u16 {
        self.0
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Supported values for DOM `WheelEvent.deltaMode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub(crate) enum WheelDeltaMode {
    Pixel = 0,
    Line = 1,
}

impl WheelDeltaMode {
    pub(crate) const fn code(self) -> u16 {
        self as u16
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NativeMouseInputKind {
    Hover,
    Move,
    Button {
        button: ChangedMouseButton,
        pressed: bool,
    },
    Wheel {
        delta_x: f64,
        delta_y: f64,
        delta_mode: WheelDeltaMode,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeMouseInput {
    pub(crate) kind: NativeMouseInputKind,
    pub(crate) hit_target: Option<NodeId>,
    pub(crate) presented_revision: u64,
    pub(crate) client_x: f64,
    pub(crate) client_y: f64,
    pub(crate) buttons: PressedMouseButtons,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeKeyboardEventKind {
    Pressed,
    Released,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeKeyboardEvent {
    pub(crate) kind: NativeKeyboardEventKind,
    pub(crate) target: NodeId,
    pub(crate) key: String,
    pub(crate) key_code: u16,
    pub(crate) repeat: bool,
    pub(crate) modifiers: Modifiers,
}

pub(super) fn native_button(button: MouseButton) -> ChangedMouseButton {
    match button {
        MouseButton::Left => ChangedMouseButton::PRIMARY,
        MouseButton::Middle => ChangedMouseButton::AUXILIARY,
        MouseButton::Right => ChangedMouseButton::SECONDARY,
        MouseButton::Other(number) => ChangedMouseButton::from_code(number),
    }
}

pub(super) fn pointer_input_for_input(
    plan: &ScenePlan,
    position: PhysicalPosition<f64>,
    buttons: u16,
    input: Option<(ElementState, MouseButton)>,
) -> NativeMouseInput {
    NativeMouseInput {
        kind: match input {
            Some((state, button)) => NativeMouseInputKind::Button {
                button: native_button(button),
                pressed: state == ElementState::Pressed,
            },
            None => NativeMouseInputKind::Move,
        },
        hit_target: plan.hit_test_physical(position.x, position.y),
        presented_revision: plan.revision(),
        client_x: position.x / plan.scale_factor(),
        client_y: position.y / plan.scale_factor(),
        buttons: PressedMouseButtons::from_bits(buttons),
    }
}

pub(super) fn wheel_input_for_input(
    plan: &ScenePlan,
    position: PhysicalPosition<f64>,
    delta_x: f64,
    delta_y: f64,
    precise: bool,
    buttons: u16,
) -> NativeMouseInput {
    let delta_scale = if precise {
        -1.0 / plan.scale_factor()
    } else {
        -1.0
    };
    NativeMouseInput {
        kind: NativeMouseInputKind::Wheel {
            delta_x: delta_x * delta_scale,
            delta_y: delta_y * delta_scale,
            delta_mode: if precise {
                WheelDeltaMode::Pixel
            } else {
                WheelDeltaMode::Line
            },
        },
        hit_target: plan.hit_test_physical(position.x, position.y),
        presented_revision: plan.revision(),
        client_x: position.x / plan.scale_factor(),
        client_y: position.y / plan.scale_factor(),
        buttons: PressedMouseButtons::from_bits(buttons),
    }
}

pub(super) fn keyboard_key(
    key_code: u16,
    text: Option<String>,
    logical_text: Option<String>,
) -> String {
    match logical_text.as_deref().or(text.as_deref()) {
        Some("\r" | "\n" | "\u{3}") => "Enter".into(),
        Some("\t") => "Tab".into(),
        Some("\u{1b}") => "Escape".into(),
        Some("\u{8}" | "\u{7f}") => "Backspace".into(),
        Some("\u{f700}") => "ArrowUp".into(),
        Some("\u{f701}") => "ArrowDown".into(),
        Some("\u{f702}") => "ArrowLeft".into(),
        Some("\u{f703}") => "ArrowRight".into(),
        Some("\u{f728}") => "Delete".into(),
        Some(text) => text.into(),
        None => match key_code {
            0x38 | 0x3c => "Shift".into(),
            0x3b | 0x3e => "Control".into(),
            0x3a | 0x3d => "Alt".into(),
            0x36 | 0x37 => "Meta".into(),
            0x39 => "CapsLock".into(),
            _ => "Unidentified".into(),
        },
    }
}

impl ApplicationHost {
    pub(super) fn queue_mouse_input(
        &mut self,
        position: PhysicalPosition<f64>,
        buttons: u16,
        input: Option<(ElementState, MouseButton)>,
    ) -> Result<(), HostError> {
        self.discard_stale_presented_frame();
        self.cursor = Some((position, buttons));
        let Some(frame) = self.presented.as_ref() else {
            if buttons == 0 {
                self.queue_pointer_cancel()?;
            }
            return Ok(());
        };
        let input = pointer_input_for_input(&frame.plan, position, buttons, input);
        let scale = frame.plan.scale_factor();
        self.queue_pointer_input(input, scale)
    }

    pub(super) fn queue_pointer_cancel(&self) -> Result<(), HostError> {
        let state = self
            .dom_bindings
            .try_borrow()
            .map_err(|_| HostError::DomBorrowConflict)?;
        match state.enqueue_pointer_cancel() {
            Ok(()) => {}
            Err(runtime::JsTaskQueueError::Full) => {
                if let Err(error) = state.enqueue_pointer_cancel_when_ready() {
                    eprintln!("Burokku warning: pointer cancellation stopped: {error}");
                }
            }
            Err(error) => eprintln!("Burokku warning: pointer cancellation stopped: {error}"),
        }
        Ok(())
    }

    pub(super) fn queue_wheel_input(
        &mut self,
        position: PhysicalPosition<f64>,
        delta_x: f64,
        delta_y: f64,
        precise: bool,
    ) -> Result<(), HostError> {
        self.discard_stale_presented_frame();
        let buttons = self.cursor.map_or(0, |(_, buttons)| buttons);
        self.cursor = Some((position, buttons));
        let Some(frame) = self.presented.as_ref() else {
            return Ok(());
        };
        let input =
            wheel_input_for_input(&frame.plan, position, delta_x, delta_y, precise, buttons);
        self.queue_pointer_input(input, frame.plan.scale_factor())
    }

    pub(super) fn queue_keyboard_input(&self, event: KeyEvent) -> Result<(), HostError> {
        let Some(target) = self.windows.current().map(|window| window.dom_id()) else {
            return Ok(());
        };
        let KeyEvent {
            key_code,
            text,
            logical_text,
            state,
            repeat,
            modifiers,
        } = event;
        let event = NativeKeyboardEvent {
            kind: match state {
                ElementState::Pressed => NativeKeyboardEventKind::Pressed,
                ElementState::Released => NativeKeyboardEventKind::Released,
            },
            target,
            key: keyboard_key(key_code, text, logical_text),
            key_code,
            repeat,
            modifiers,
        };
        let state = self
            .dom_bindings
            .try_borrow()
            .map_err(|_| HostError::DomBorrowConflict)?;
        if let Err(error) = state.enqueue_keyboard_event(event) {
            eprintln!("Burokku warning: dropped keyboard input: {error}");
        }
        Ok(())
    }

    pub(super) fn queue_hover_at_cursor(&mut self) -> Result<(), HostError> {
        self.discard_stale_presented_frame();
        let (Some((position, buttons)), Some(frame)) = (self.cursor, self.presented.as_ref())
        else {
            return Ok(());
        };
        let scale = frame.plan.scale_factor();
        self.queue_pointer_input(
            NativeMouseInput {
                kind: NativeMouseInputKind::Hover,
                hit_target: frame.plan.hit_test_physical(position.x, position.y),
                presented_revision: frame.revision(),
                client_x: position.x / scale,
                client_y: position.y / scale,
                buttons: PressedMouseButtons::from_bits(buttons),
            },
            scale,
        )
    }

    pub(super) fn queue_cursor_exit(
        &mut self,
        position: PhysicalPosition<f64>,
        buttons: u16,
    ) -> Result<(), HostError> {
        if buttons == 0 {
            self.queue_pointer_cancel()?;
        }
        self.cursor = None;
        let scale = self
            .windows
            .current()
            .ok_or(HostError::MissingNativeWindow)?
            .window()
            .scale_factor();
        let revision = self
            .dom_bindings
            .try_borrow()
            .map_err(|_| HostError::DomBorrowConflict)?
            .dom
            .revision();
        self.queue_pointer_input(
            NativeMouseInput {
                kind: NativeMouseInputKind::Hover,
                hit_target: None,
                presented_revision: revision,
                client_x: position.x / scale,
                client_y: position.y / scale,
                buttons: PressedMouseButtons::from_bits(buttons),
            },
            scale,
        )
    }

    pub(super) fn queue_pointer_input(
        &mut self,
        input: NativeMouseInput,
        scale: f64,
    ) -> Result<(), HostError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(HostError::InvalidScaleFactor(scale));
        }
        if !input.client_x.is_finite() || !input.client_y.is_finite() {
            return Ok(());
        }
        let state = self
            .dom_bindings
            .try_borrow()
            .map_err(|_| HostError::DomBorrowConflict)?;
        let enqueue = state.enqueue_mouse_input(input);
        drop(state);
        if let Err(error) = enqueue {
            if matches!(
                input.kind,
                NativeMouseInputKind::Button { pressed: false, .. }
            ) && input.buttons.is_empty()
            {
                eprintln!(
                    "Burokku warning: replacing dropped pointer release with cancellation: {error}"
                );
                self.queue_pointer_cancel()?;
            } else {
                eprintln!("Burokku warning: dropped pointer input: {error}");
            }
        }
        Ok(())
    }
}

impl ApplicationHandler for ApplicationHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.sync_dom(event_loop) {
            self.fail(event_loop, error);
            return;
        }

        // A ready renderer needs a host-owned frame after resume. First-window
        // initialization requests its redraw asynchronously on completion.
        if self.renderer.is_some() {
            if let Some(window) = self.windows.current() {
                window.window().request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .windows
            .current()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Err(error) = self.queue_pointer_cancel() {
                    self.fail(event_loop, error);
                    return;
                }
                self.cancel_graphics_initialization();
                self.renderer = None;
                self.presented = None;
                self.dom_bindings.borrow_mut().clear_hover_path();
                self.cursor = None;
                self.windows.close();
                self.request_exit();
            }
            WindowEvent::Resized(size)
            | WindowEvent::ScaleFactorChanged {
                new_inner_size: size,
                ..
            } => {
                if self.pending_graphics.is_some() {
                    return;
                }
                let dom_bindings = Rc::clone(&self.dom_bindings);
                let revision = match dom_bindings.try_borrow() {
                    Ok(state) => state.dom.revision(),
                    Err(_) => {
                        self.fail(event_loop, HostError::DomBorrowConflict);
                        return;
                    }
                };
                let Some(graphics) = self.graphics.as_ref() else {
                    self.fail(event_loop, HostError::MissingGraphicsContext);
                    return;
                };
                let Some(renderer) = self.renderer.as_mut() else {
                    self.fail(event_loop, HostError::MissingRenderer);
                    return;
                };
                let resize_result = renderer.resize(graphics, size);
                let active_surface = (window_id == renderer.window_id()
                    && size == renderer.physical_size())
                .then_some(PresentedSurface {
                    window_id,
                    physical_size: renderer.physical_size(),
                    generation: renderer.surface_generation(),
                });
                let presentation = presentation_state(
                    self.presented.as_ref(),
                    active_surface.as_ref(),
                    renderer.last_presented_revision(),
                );
                if !presentation.usable_frame {
                    self.presented = None;
                }
                if let Err(error) = resize_result {
                    let failure = classify_resize_failure(
                        revision,
                        presentation.active_renderer_has_presented,
                        error,
                    );
                    self.handle_redraw_failure(event_loop, failure);
                    return;
                }
                if size.width > 0 && size.height > 0 {
                    if let Some(window) = self.windows.current() {
                        window.window().request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.sync_dom(event_loop) {
                    self.fail(event_loop, error);
                    return;
                }
                if self.pending_graphics.is_some()
                    || self.pending_graphics_replacement.is_some()
                    || self
                        .windows
                        .current()
                        .is_none_or(|window| window.id() != window_id)
                {
                    return;
                }
                match self.redraw() {
                    Ok(PresentationOutcome::Reconfigure) => {
                        self.discard_stale_presented_frame();
                        if let Some(window) = self.windows.current() {
                            window.window().request_redraw();
                        }
                    }
                    Ok(PresentationOutcome::Timeout) => {
                        if let Some(window) = self.windows.current() {
                            window.window().request_redraw();
                        }
                    }
                    Ok(PresentationOutcome::Presented { .. } | PresentationOutcome::Occluded) => {}
                    Err(failure) => {
                        // Recoverable candidate failures retain the Window,
                        // renderer, and presented hit-test plan. Do not request an
                        // immediate retry for the unchanged revision/viewport.
                        self.handle_redraw_failure(event_loop, failure);
                    }
                }
            }
            WindowEvent::CursorEntered { position, buttons } => {
                self.discard_stale_presented_frame();
                self.cursor = Some((position, buttons));
                if let Err(error) = self.queue_hover_at_cursor() {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::CursorLeft { position, buttons } => {
                if let Err(error) = self.queue_cursor_exit(position, buttons) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::CursorMoved { position, buttons } => {
                if let Err(error) = self.queue_mouse_input(position, buttons, None) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::MouseInput {
                state,
                button,
                position,
                buttons,
            } => {
                if let Err(error) = self.queue_mouse_input(position, buttons, Some((state, button)))
                {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::MouseWheel {
                delta_x,
                delta_y,
                precise,
                position,
            } => {
                if let Err(error) = self.queue_wheel_input(position, delta_x, delta_y, precise) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::Occluded(false) => {
                // On macOS, WGPU can reject the first surface texture as
                // occluded while AppKit is still making a newly shown window
                // visible. Retry once AppKit confirms visibility; otherwise
                // the next redraw may not arrive until the user resizes.
                if let Some(window) = self.windows.current() {
                    window.window().request_redraw();
                }
            }
            WindowEvent::KeyboardInput(event) => {
                if let Err(error) = self.queue_keyboard_input(event) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::Focused(false) => {
                if let Err(error) = self.queue_pointer_cancel() {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::Focused(true)
            | WindowEvent::Occluded(true)
            | WindowEvent::ModifiersChanged(_) => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let runtime_status = self.lifecycle.status();
        if matches!(runtime_status, RuntimeStatus::Failed(_)) {
            event_loop.exit();
            return;
        }

        if self.fatal_error.is_some() {
            self.request_exit();
        }
        if self.exit_requested {
            if runtime_status == RuntimeStatus::Stopped {
                event_loop.exit();
            } else {
                self.lifecycle.request_shutdown();
            }
            return;
        }

        let dom_bindings = Rc::clone(&self.dom_bindings);
        let reclaimed = {
            let mut state = match dom_bindings.try_borrow_mut() {
                Ok(state) => state,
                Err(_) => {
                    self.fail(event_loop, HostError::DomBorrowConflict);
                    return;
                }
            };
            match state.reclaim_detached() {
                Ok(report) => report.nodes,
                Err(error) => {
                    drop(state);
                    self.fail(event_loop, HostError::DomMaintenance(error.to_string()));
                    return;
                }
            }
        };
        self.layout.remove_nodes(&reclaimed);

        if let Err(error) = self
            .sync_dom(event_loop)
            .and_then(|()| self.complete_graphics_initialization())
            .and_then(|()| self.complete_graphics_replacement())
            .and_then(|()| self.sync_dom(event_loop))
        {
            self.fail(event_loop, error);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.lifecycle.request_shutdown();
        self.cancel_graphics_initialization();
        self.renderer = None;
        self.windows.close();
    }
}
