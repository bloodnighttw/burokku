//! Platform-neutral DOM event data, dispatch planning, and interaction state.

use crate::ui::elements::{Dom, DomError, NodeId};

mod keyboard;
mod mouse;
mod pointer;

pub(crate) use keyboard::{DomKeyboardEvent, KeyboardEventKind};
pub(crate) use mouse::{DomMouseEvent, MouseEventKind};
pub(crate) use pointer::{DomPointerEvent, PointerEventKind, PointerState};

/// Platform-neutral work required to dispatch one DOM event.
///
/// The path and event behavior are resolved before entering the JavaScript executor.
/// The executor walks `path` in order and may stop early when a listener requests it.
pub(crate) struct DispatchPlan<E> {
    pub(crate) event: E,
    pub(crate) path: Vec<NodeId>,
    pub(crate) bubbles: bool,
    pub(crate) cancelable: bool,
}

fn build_dispatch_plan<E>(
    dom: &Dom,
    event: E,
    target: NodeId,
    bubbles: bool,
    cancelable: bool,
) -> Result<Option<DispatchPlan<E>>, DomError> {
    if !dom.is_connected(target).unwrap_or(false) {
        return Ok(None);
    }
    let mut path = Vec::new();
    let mut current = Some(target);
    while let Some(id) = current {
        path.push(id);
        if !bubbles {
            break;
        }
        current = dom.parent_node(id)?;
    }
    Ok(Some(DispatchPlan {
        event,
        path,
        bubbles,
        cancelable,
    }))
}
