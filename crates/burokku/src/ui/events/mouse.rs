use crate::ui::{
    elements::NodeId,
    host::{ChangedMouseButton, PressedMouseButtons, WheelDeltaMode},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MouseEventKind {
    Click,
    Wheel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DomMouseEvent {
    pub(crate) kind: MouseEventKind,
    pub(crate) target: NodeId,
    pub(crate) presented_revision: u64,
    pub(crate) client_x: f64,
    pub(crate) client_y: f64,
    pub(crate) button: ChangedMouseButton,
    pub(crate) buttons: PressedMouseButtons,
    pub(crate) wheel_delta: Option<(f64, f64, WheelDeltaMode)>,
}
