use crate::ui::{
    elements::NodeId,
    host::{
        ChangedMouseButton, NativeMouseInput, NativeMouseInputKind, PressedMouseButtons,
        WheelDeltaMode,
    },
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

impl DomMouseEvent {
    pub(crate) fn wheel(input: NativeMouseInput, target: NodeId) -> Option<Self> {
        let NativeMouseInputKind::Wheel {
            delta_x,
            delta_y,
            delta_mode,
        } = input.kind
        else {
            return None;
        };
        Some(Self {
            kind: MouseEventKind::Wheel,
            target,
            presented_revision: input.presented_revision,
            client_x: input.client_x,
            client_y: input.client_y,
            button: ChangedMouseButton::PRIMARY,
            buttons: input.buttons,
            wheel_delta: Some((delta_x, delta_y, delta_mode)),
        })
    }

    pub(crate) fn click(input: NativeMouseInput, target: NodeId) -> Self {
        Self {
            kind: MouseEventKind::Click,
            target,
            presented_revision: input.presented_revision,
            client_x: input.client_x,
            client_y: input.client_y,
            button: ChangedMouseButton::PRIMARY,
            buttons: input.buttons,
            wheel_delta: None,
        }
    }
}
