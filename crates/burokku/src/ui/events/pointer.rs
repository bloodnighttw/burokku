use crate::ui::{
    elements::NodeId,
    host::{ChangedMouseButton, PressedMouseButtons},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerEventKind {
    Down,
    Up,
    Move,
    Enter,
    Leave,
    Cancel,
    GotCapture,
    LostCapture,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DomPointerEvent {
    pub(crate) kind: PointerEventKind,
    pub(crate) target: NodeId,
    pub(crate) presented_revision: u64,
    pub(crate) client_x: f64,
    pub(crate) client_y: f64,
    pub(crate) button: ChangedMouseButton,
    pub(crate) buttons: PressedMouseButtons,
    pub(crate) related_target: Option<NodeId>,
    pub(crate) pointer_id: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivePointer {
    pub(crate) target: NodeId,
    pub(crate) presented_revision: u64,
    pub(crate) client_x: f64,
    pub(crate) client_y: f64,
}

#[derive(Debug, Default)]
pub(crate) struct PointerState {
    pub(crate) active: Option<ActivePointer>,
    pub(crate) pressed_target: Option<NodeId>,
    pub(crate) capture: Option<NodeId>,
    pub(crate) announced_capture: Option<NodeId>,
    pub(crate) hover_path: Vec<NodeId>,
}
