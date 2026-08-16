use crate::{PhysicalPosition, PhysicalSize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElementState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
    pub caps_lock: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    /// The platform's numeric virtual key code.
    pub key_code: u16,
    /// Text after applying the current keyboard layout, when available.
    pub text: Option<String>,
    pub state: ElementState,
    pub repeat: bool,
    pub modifiers: Modifiers,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WindowEvent {
    CloseRequested,
    Resized(PhysicalSize<u32>),
    ScaleFactorChanged {
        scale_factor: f64,
        new_inner_size: PhysicalSize<u32>,
    },
    RedrawRequested,
    Focused(bool),
    Occluded(bool),
    KeyboardInput(KeyEvent),
    ModifiersChanged(Modifiers),
    CursorMoved {
        position: PhysicalPosition<f64>,
    },
    MouseInput {
        state: ElementState,
        button: MouseButton,
        position: PhysicalPosition<f64>,
    },
    MouseWheel {
        delta_x: f64,
        delta_y: f64,
        precise: bool,
        position: PhysicalPosition<f64>,
    },
}
