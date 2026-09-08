use crate::ui::{
    elements::{Dom, DomError, NodeId},
    host::{ChangedMouseButton, NativeMouseInput, NativeMouseInputKind, PressedMouseButtons},
};

use super::{build_dispatch_plan, DispatchPlan, DomMouseEvent};

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

impl DomPointerEvent {
    fn from_input(input: NativeMouseInput, target: NodeId) -> Option<Self> {
        let (kind, button) = match input.kind {
            NativeMouseInputKind::Hover | NativeMouseInputKind::Wheel { .. } => return None,
            NativeMouseInputKind::Move => (PointerEventKind::Move, ChangedMouseButton::NONE),
            NativeMouseInputKind::Button { button, pressed } => {
                let changed_button = button.buttons_bit();
                if changed_button == 0 {
                    return None;
                }
                let kind = if pressed && input.buttons.bits() == changed_button {
                    PointerEventKind::Down
                } else if !pressed && input.buttons.is_empty() {
                    PointerEventKind::Up
                } else {
                    PointerEventKind::Move
                };
                (kind, button)
            }
        };
        Some(Self {
            kind,
            target,
            presented_revision: input.presented_revision,
            client_x: input.client_x,
            client_y: input.client_y,
            button,
            buttons: input.buttons,
            related_target: None,
            pointer_id: 1,
        })
    }

    pub(crate) fn uses_pointer_state(self) -> bool {
        matches!(
            self.kind,
            PointerEventKind::Down
                | PointerEventKind::Up
                | PointerEventKind::Move
                | PointerEventKind::Cancel
        )
    }

    pub(crate) fn plan_dispatch(
        mut self,
        dom: &Dom,
    ) -> Result<Option<DispatchPlan<Self>>, DomError> {
        let bubbles = !matches!(self.kind, PointerEventKind::Enter | PointerEventKind::Leave);
        let cancelable = bubbles
            && !matches!(
                self.kind,
                PointerEventKind::Cancel
                    | PointerEventKind::GotCapture
                    | PointerEventKind::LostCapture
            );
        self.related_target = self
            .related_target
            .filter(|target| dom.node(*target).is_some());
        let target = self.target;
        build_dispatch_plan(dom, self, target, bubbles, cancelable)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ActivePointer {
    pub(crate) target: NodeId,
    pub(crate) presented_revision: u64,
    pub(crate) client_x: f64,
    pub(crate) client_y: f64,
}

/// Stateful DOM pointer bookkeeping shared across native input events.
///
/// It remembers enough history to derive hover boundaries and clicks, retarget
/// events through pointer capture, and emit capture lifecycle events. It contains
/// no JavaScript state; [`PointerState::plan_input`] turns it into typed event plans.
#[derive(Debug, Default)]
pub(crate) struct PointerState {
    /// Pointer currently between `pointerdown` and its final `pointerup` or cancellation.
    pub(crate) active: Option<ActivePointer>,
    /// Primary-button target retained to synthesize `click` on release over the same target.
    pub(crate) pressed_target: Option<NodeId>,
    /// Connected node currently requested as the pointer-capture target.
    pub(crate) capture: Option<NodeId>,
    /// Last capture target reported through `gotpointercapture`/`lostpointercapture`.
    pub(crate) announced_capture: Option<NodeId>,
    /// Hovered nodes ordered from the hit target toward the DOM root.
    pub(crate) hover_path: Vec<NodeId>,
}

/// Dispatch work produced by [`PointerState::plan_input`] for one native input.
///
/// The JavaScript executor consumes the fields in this order: boundary events
/// first, then the main pointer event, then the derived mouse event. A field is
/// empty when that input does not produce that event family—for example, wheel
/// input produces boundary changes and a mouse event, but no pointer event.
pub(crate) struct PointingInputPlan {
    /// `pointerleave` and `pointerenter` events caused by a hover-path change.
    pub(crate) boundaries: Vec<DomPointerEvent>,
    /// The primary `pointerdown`, `pointerup`, or `pointermove` event.
    pub(crate) pointer: Option<DomPointerEvent>,
    /// A synthesized `click` or translated `wheel` event.
    pub(crate) mouse: Option<DomMouseEvent>,
}

impl PointerState {
    pub(crate) fn capture_target(&self, dom: &Dom) -> Option<NodeId> {
        self.capture
            .filter(|target| dom.is_connected(*target).unwrap_or(false))
    }

    pub(crate) fn clear_disconnected_capture(&mut self, dom: &Dom) {
        if self.capture_target(dom).is_none() {
            self.capture = None;
        }
    }

    pub(crate) fn clear_hover_path(&mut self) {
        self.hover_path.clear();
    }

    #[cfg(test)]
    pub(crate) fn hover_path(&self) -> &[NodeId] {
        &self.hover_path
    }

    pub(crate) fn plan_input(&mut self, dom: &Dom, input: NativeMouseInput) -> PointingInputPlan {
        let boundaries = if self.capture_target(dom).is_some() {
            Vec::new()
        } else {
            let next = hover_path(dom, input.hit_target);
            let events = hover_events(&self.hover_path, &next, input);
            self.hover_path = next;
            events
        };
        let target = if matches!(
            input.kind,
            NativeMouseInputKind::Move | NativeMouseInputKind::Button { .. }
        ) {
            self.capture_target(dom).or(input.hit_target)
        } else {
            input.hit_target
        };
        let click_target = match input.kind {
            NativeMouseInputKind::Button {
                button: ChangedMouseButton::PRIMARY,
                pressed: true,
            } => {
                self.pressed_target = target;
                None
            }
            NativeMouseInputKind::Button {
                button: ChangedMouseButton::PRIMARY,
                pressed: false,
            } => self
                .pressed_target
                .take()
                .filter(|pressed| Some(*pressed) == target),
            NativeMouseInputKind::Move if input.buttons.bits() & 1 == 0 => {
                self.pressed_target = None;
                None
            }
            _ => None,
        };
        PointingInputPlan {
            boundaries,
            pointer: target.and_then(|target| DomPointerEvent::from_input(input, target)),
            mouse: click_target
                .map(|target| DomMouseEvent::click(input, target))
                .or_else(|| target.and_then(|target| DomMouseEvent::wheel(input, target))),
        }
    }

    pub(crate) fn cancel_event(&self) -> Option<DomPointerEvent> {
        self.active.map(|active| DomPointerEvent {
            kind: PointerEventKind::Cancel,
            target: active.target,
            presented_revision: active.presented_revision,
            client_x: active.client_x,
            client_y: active.client_y,
            button: ChangedMouseButton::NONE,
            buttons: PressedMouseButtons::NONE,
            related_target: None,
            pointer_id: 1,
        })
    }

    pub(crate) fn start_dispatch(
        &mut self,
        dom: &Dom,
        mut event: DomPointerEvent,
    ) -> (Vec<DomPointerEvent>, DomPointerEvent) {
        match event.kind {
            PointerEventKind::Down => {
                self.active = Some(ActivePointer {
                    target: event.target,
                    presented_revision: event.presented_revision,
                    client_x: event.client_x,
                    client_y: event.client_y,
                });
            }
            PointerEventKind::Move | PointerEventKind::Up => {
                if let Some(active) = self.active.as_mut() {
                    active.presented_revision = event.presented_revision;
                    active.client_x = event.client_x;
                    active.client_y = event.client_y;
                }
            }
            _ => {}
        }
        let transitions = self.capture_transitions(dom, event);
        if let Some(target) = self.capture_target(dom) {
            event.target = target;
        }
        (transitions, event)
    }

    pub(crate) fn finish_dispatch(
        &mut self,
        dom: &Dom,
        event: DomPointerEvent,
    ) -> Vec<DomPointerEvent> {
        if event.kind == PointerEventKind::Cancel
            || (event.kind == PointerEventKind::Up && event.buttons.is_empty())
        {
            self.active = None;
            self.pressed_target = None;
            self.capture = None;
        }
        self.capture_transitions(dom, event)
    }

    fn capture_transitions(&mut self, dom: &Dom, source: DomPointerEvent) -> Vec<DomPointerEvent> {
        self.clear_disconnected_capture(dom);
        let previous = self.announced_capture;
        let next = self.capture;
        if previous == next {
            return Vec::new();
        }
        self.announced_capture = next;
        [
            (PointerEventKind::LostCapture, previous),
            (PointerEventKind::GotCapture, next),
        ]
        .into_iter()
        .filter_map(|(kind, target)| {
            Some(DomPointerEvent {
                kind,
                target: target?,
                related_target: None,
                ..source
            })
        })
        .collect()
    }
}

fn hover_path(dom: &Dom, target: Option<NodeId>) -> Vec<NodeId> {
    let mut path = Vec::new();
    let mut current = target.filter(|id| dom.is_connected(*id).unwrap_or(false));
    while let Some(id) = current {
        path.push(id);
        current = dom.parent_node(id).expect("hover path contains live nodes");
    }
    path
}

fn hover_events(
    previous: &[NodeId],
    next: &[NodeId],
    input: NativeMouseInput,
) -> Vec<DomPointerEvent> {
    if previous == next {
        return Vec::new();
    }
    let mut events = Vec::new();
    let button = match input.kind {
        NativeMouseInputKind::Button { button, .. } => button,
        NativeMouseInputKind::Wheel { .. } => ChangedMouseButton::PRIMARY,
        NativeMouseInputKind::Hover | NativeMouseInputKind::Move => ChangedMouseButton::NONE,
    };
    let mut push = |kind, target, related_target| {
        events.push(DomPointerEvent {
            kind,
            target,
            related_target,
            presented_revision: input.presented_revision,
            client_x: input.client_x,
            client_y: input.client_y,
            button,
            buttons: input.buttons,
            pointer_id: 1,
        });
    };
    let old_target = previous.first().copied();
    let new_target = next.first().copied();
    // ponytail: O(depth²) membership checks; use sets if deep hover paths become costly.
    // Comparing membership also handles a still-hovered subtree being reparented.
    for &node in previous.iter().filter(|node| !next.contains(node)) {
        push(PointerEventKind::Leave, node, new_target);
    }
    for &node in next.iter().rev().filter(|node| !previous.contains(node)) {
        push(PointerEventKind::Enter, node, old_target);
    }
    events
}
