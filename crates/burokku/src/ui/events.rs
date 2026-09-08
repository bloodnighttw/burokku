//! Platform-neutral DOM event data and interaction state.

mod keyboard;
mod mouse;
mod pointer;

pub(crate) use keyboard::{DomKeyboardEvent, KeyboardEventKind};
pub(crate) use mouse::{DomMouseEvent, MouseEventKind};
pub(crate) use pointer::{DomPointerEvent, PointerEventKind, PointerState};
