use crate::ui::{
    elements::{Dom, DomError, NodeId},
    host::{NativeKeyboardEvent, NativeKeyboardEventKind},
};

use super::{build_dispatch_plan, DispatchPlan};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeyboardEventKind {
    Down,
    Up,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DomKeyboardEvent {
    pub(crate) kind: KeyboardEventKind,
    pub(crate) target: NodeId,
    pub(crate) key: String,
    pub(crate) key_code: u16,
    pub(crate) repeat: bool,
    pub(crate) shift_key: bool,
    pub(crate) ctrl_key: bool,
    pub(crate) alt_key: bool,
    pub(crate) meta_key: bool,
}

impl DomKeyboardEvent {
    pub(crate) fn plan_dispatch(self, dom: &Dom) -> Result<Option<DispatchPlan<Self>>, DomError> {
        let target = self.target;
        build_dispatch_plan(dom, self, target, true, true)
    }
}

impl From<NativeKeyboardEvent> for DomKeyboardEvent {
    fn from(event: NativeKeyboardEvent) -> Self {
        Self {
            kind: match event.kind {
                NativeKeyboardEventKind::Pressed => KeyboardEventKind::Down,
                NativeKeyboardEventKind::Released => KeyboardEventKind::Up,
            },
            target: event.target,
            key: event.key,
            key_code: event.key_code,
            repeat: event.repeat,
            shift_key: event.modifiers.shift,
            ctrl_key: event.modifiers.control,
            alt_key: event.modifiers.alt,
            meta_key: event.modifiers.command,
        }
    }
}
