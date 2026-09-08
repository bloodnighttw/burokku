//! Pointer input, hover, capture, and click synthesis.

use rquickjs::{Class, Ctx, Object, Result as JsResult};

use super::super::{
    classes::{borrow, borrow_mut, NativeNode},
    DomBindingState, SharedDomBindings,
};
use super::mouse::{dispatch_mouse_event_inner, NativeMouseEvent};
use crate::ui::{
    elements::NodeId,
    host::{ChangedMouseButton, NativeMouseInput, NativeMouseInputKind, PressedMouseButtons},
};

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::dom_plugin) struct ActivePointer {
    target: NodeId,
    presented_revision: u64,
    client_x: f64,
    client_y: f64,
}

#[derive(Debug, Default)]
pub(in crate::ui::dom_plugin) struct PointerState {
    pub(in crate::ui::dom_plugin) active: Option<ActivePointer>,
    pub(in crate::ui::dom_plugin) pressed_target: Option<NodeId>,
    pub(in crate::ui::dom_plugin) capture: Option<NodeId>,
    pub(in crate::ui::dom_plugin) announced_capture: Option<NodeId>,
    pub(in crate::ui::dom_plugin) hover_path: Vec<NodeId>,
}

impl DomBindingState {
    pub(crate) fn pointer_capture_target(&self) -> Option<NodeId> {
        self.pointer
            .capture
            .filter(|target| self.dom.is_connected(*target).unwrap_or(false))
    }

    pub(in crate::ui::dom_plugin) fn clear_disconnected_pointer_capture(&mut self) {
        if self.pointer_capture_target().is_none() {
            self.pointer.capture = None;
        }
    }

    pub(crate) fn clear_hover_path(&mut self) {
        self.pointer.hover_path.clear();
    }

    #[cfg(test)]
    pub(crate) fn hover_path(&self) -> &[NodeId] {
        &self.pointer.hover_path
    }
}

fn hover_path(state: &DomBindingState, target: Option<NodeId>) -> Vec<NodeId> {
    let mut path = Vec::new();
    let mut current = target.filter(|id| state.dom.is_connected(*id).unwrap_or(false));
    while let Some(id) = current {
        path.push(id);
        current = state
            .dom
            .parent_node(id)
            .expect("hover path contains live nodes");
    }
    path
}

fn mouse_event(input: NativeMouseInput, target: NodeId) -> Option<NativeMouseEvent> {
    let (event_type, button, wheel_delta, pointer_id) = match input.kind {
        NativeMouseInputKind::Hover => return None,
        NativeMouseInputKind::Move => ("pointermove", ChangedMouseButton::NONE, None, Some(1)),
        NativeMouseInputKind::Button { button, pressed } => {
            let changed_button = button.buttons_bit();
            if changed_button == 0 {
                return None;
            }
            let event_type = if pressed && input.buttons.bits() == changed_button {
                "pointerdown"
            } else if !pressed && input.buttons.is_empty() {
                "pointerup"
            } else {
                "pointermove"
            };
            (event_type, button, None, Some(1))
        }
        NativeMouseInputKind::Wheel {
            delta_x,
            delta_y,
            delta_mode,
        } => (
            "wheel",
            ChangedMouseButton::PRIMARY,
            Some((delta_x, delta_y, delta_mode)),
            None,
        ),
    };
    Some(NativeMouseEvent {
        event_type,
        target,
        presented_revision: input.presented_revision,
        client_x: input.client_x,
        client_y: input.client_y,
        button,
        buttons: input.buttons,
        related_target: None,
        wheel_delta,
        pointer_id,
    })
}

fn hover_events(
    previous: &[NodeId],
    next: &[NodeId],
    input: NativeMouseInput,
) -> Vec<NativeMouseEvent> {
    if previous == next {
        return Vec::new();
    }
    let mut events = Vec::new();
    let button = match input.kind {
        NativeMouseInputKind::Button { button, .. } => button,
        NativeMouseInputKind::Wheel { .. } => ChangedMouseButton::PRIMARY,
        NativeMouseInputKind::Hover | NativeMouseInputKind::Move => ChangedMouseButton::NONE,
    };
    let mut push = |event_type, target, related_target| {
        events.push(NativeMouseEvent {
            event_type,
            target,
            related_target,
            presented_revision: input.presented_revision,
            client_x: input.client_x,
            client_y: input.client_y,
            button,
            buttons: input.buttons,
            wheel_delta: None,
            pointer_id: Some(1),
        });
    };
    let old_target = previous.first().copied();
    let new_target = next.first().copied();
    // ponytail: O(depth²) membership checks; use sets if deep hover paths become costly.
    // Comparing membership also handles a still-hovered subtree being reparented.
    for &node in previous.iter().filter(|node| !next.contains(node)) {
        push("pointerleave", node, new_target);
    }
    for &node in next.iter().rev().filter(|node| !previous.contains(node)) {
        push("pointerenter", node, old_target);
    }
    events
}

pub(super) fn dispatch_mouse_input(context: &Ctx<'_>, input: NativeMouseInput) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let boundaries = {
        let mut state = borrow_mut(context, &state)?;
        if state.pointer_capture_target().is_some() {
            Vec::new()
        } else {
            let next = hover_path(&state, input.hit_target);
            let events = hover_events(&state.pointer.hover_path, &next, input);
            state.pointer.hover_path = next;
            events
        }
    };
    for event in boundaries {
        dispatch_mouse_event(context, event)?;
    }

    let target = {
        let state = borrow(context, &state)?;
        if matches!(
            input.kind,
            NativeMouseInputKind::Move | NativeMouseInputKind::Button { .. }
        ) {
            state.pointer_capture_target().or(input.hit_target)
        } else {
            input.hit_target
        }
    };
    let click_target = {
        let mut state = borrow_mut(context, &state)?;
        match input.kind {
            NativeMouseInputKind::Button {
                button: ChangedMouseButton::PRIMARY,
                pressed: true,
            } => {
                state.pointer.pressed_target = target;
                None
            }
            NativeMouseInputKind::Button {
                button: ChangedMouseButton::PRIMARY,
                pressed: false,
            } => state
                .pointer
                .pressed_target
                .take()
                .filter(|pressed| Some(*pressed) == target),
            NativeMouseInputKind::Move if input.buttons.bits() & 1 == 0 => {
                state.pointer.pressed_target = None;
                None
            }
            _ => None,
        }
    };

    if let Some(event) = target.and_then(|target| mouse_event(input, target)) {
        dispatch_mouse_event(context, event)?;
    }

    if let Some(target) = click_target {
        dispatch_mouse_event(
            context,
            NativeMouseEvent {
                event_type: "click",
                target,
                presented_revision: input.presented_revision,
                client_x: input.client_x,
                client_y: input.client_y,
                button: ChangedMouseButton::PRIMARY,
                buttons: input.buttons,
                related_target: None,
                wheel_delta: None,
                pointer_id: None,
            },
        )?;
    }
    Ok(())
}
pub(super) fn dispatch_pointer_cancel(context: &Ctx<'_>) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let Some(active) = borrow(context, &state)?.pointer.active else {
        return Ok(());
    };
    dispatch_mouse_event(
        context,
        NativeMouseEvent {
            event_type: "pointercancel",
            target: active.target,
            presented_revision: active.presented_revision,
            client_x: active.client_x,
            client_y: active.client_y,
            button: ChangedMouseButton::NONE,
            buttons: PressedMouseButtons::NONE,
            related_target: None,
            wheel_delta: None,
            pointer_id: Some(1),
        },
    )
}

pub(in crate::ui::dom_plugin) fn dispatch_mouse_event(
    context: &Ctx<'_>,
    mut mouse: NativeMouseEvent,
) -> JsResult<()> {
    let routes_through_capture = matches!(
        mouse.event_type,
        "pointerdown" | "pointerup" | "pointermove" | "pointercancel"
    );
    if !routes_through_capture {
        return dispatch_mouse_event_inner(context, mouse);
    }

    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    {
        let mut state = borrow_mut(context, &state)?;
        match mouse.event_type {
            "pointerdown" => {
                state.pointer.active = Some(ActivePointer {
                    target: mouse.target,
                    presented_revision: mouse.presented_revision,
                    client_x: mouse.client_x,
                    client_y: mouse.client_y,
                });
            }
            "pointermove" | "pointerup" => {
                if let Some(active) = state.pointer.active.as_mut() {
                    active.presented_revision = mouse.presented_revision;
                    active.client_x = mouse.client_x;
                    active.client_y = mouse.client_y;
                }
            }
            _ => {}
        }
    }
    dispatch_pointer_capture_transitions(context, &state, mouse)?;
    if let Some(target) = borrow(context, &state)?.pointer.capture {
        mouse.target = target;
    }

    let result = dispatch_mouse_event_inner(context, mouse);
    if mouse.event_type == "pointercancel"
        || (mouse.event_type == "pointerup" && mouse.buttons.is_empty())
    {
        let mut state = borrow_mut(context, &state)?;
        state.pointer.active = None;
        state.pointer.pressed_target = None;
        state.pointer.capture = None;
    }
    let transitions = dispatch_pointer_capture_transitions(context, &state, mouse);
    result?;
    transitions
}

fn dispatch_pointer_capture_transitions(
    context: &Ctx<'_>,
    state: &SharedDomBindings,
    source: NativeMouseEvent,
) -> JsResult<()> {
    let transitions = {
        let mut state = borrow_mut(context, state)?;
        if state
            .pointer
            .capture
            .is_some_and(|target| !state.dom.is_connected(target).unwrap_or(false))
        {
            state.pointer.capture = None;
        }
        let previous = state.pointer.announced_capture;
        let next = state.pointer.capture;
        if previous == next {
            return Ok(());
        }
        state.pointer.announced_capture = next;
        [
            ("lostpointercapture", previous),
            ("gotpointercapture", next),
        ]
    };

    for (event_type, target) in transitions {
        let Some(target) = target else {
            continue;
        };
        dispatch_mouse_event_inner(
            context,
            NativeMouseEvent {
                event_type,
                target,
                related_target: None,
                wheel_delta: None,
                pointer_id: Some(1),
                ..source
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use runtime::{
        rquickjs::{Context, Runtime as JsRuntime},
        Plugin,
    };

    use super::*;
    use crate::ui::{
        dom_plugin::DomPlugin,
        layout::{LayoutEngine, LogicalViewport},
        text::TextEngine,
    };

    fn context() -> (JsRuntime, Context) {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        (runtime, context)
    }

    #[test]
    fn pointer_capture_retargets_and_releases() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();
        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    r#"
                globalThis.captureWindow = app.createElement('window');
                globalThis.hitTarget = app.createElement('div');
                globalThis.captureTarget = app.createElement('div');
                captureWindow.appendChild(hitTarget);
                captureWindow.appendChild(captureTarget);
                app.appendChild(captureWindow);
                globalThis.captureLog = [];
                hitTarget.addEventListener('pointerdown', event => {
                    captureLog.push('down:hit');
                    captureTarget.setPointerCapture(event.pointerId);
                    captureLog.push(`set:${captureTarget.hasPointerCapture(1)}`);
                });
                captureTarget.addEventListener('gotpointercapture', event => {
                    event.preventDefault();
                    captureLog.push(`got:${event.target === captureTarget}:${event.cancelable}:${event.defaultPrevented}`);
                });
                captureTarget.addEventListener('pointermove', event => {
                    captureLog.push(`move:capture`);
                    captureTarget.releasePointerCapture(event.pointerId);
                    captureLog.push(`release:${captureTarget.hasPointerCapture(1)}`);
                });
                captureTarget.addEventListener('lostpointercapture', event => {
                    event.preventDefault();
                    captureLog.push(`lost:${event.target === captureTarget}:${captureTarget.hasPointerCapture(1)}:${event.defaultPrevented}`);
                });
                hitTarget.addEventListener('pointermove', event => {
                    captureLog.push('move:hit');
                    captureTarget.setPointerCapture(event.pointerId);
                });
                captureTarget.addEventListener('pointerup', () => {
                    captureLog.push(`up:${captureTarget.hasPointerCapture(1)}`);
                });
            "#,
                )
                .unwrap();
            let (hit_target, capture_target, revision) = {
                let state = plugin.state();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                let children = state.dom.children(window).unwrap();
                (children[0], children[1], state.dom.revision())
            };
            let pointer = |event_type, buttons| NativeMouseEvent {
                event_type,
                target: hit_target,
                presented_revision: revision,
                client_x: 10.0,
                client_y: 20.0,
                button: ChangedMouseButton::PRIMARY,
                buttons: PressedMouseButtons::from_bits(buttons),
                related_target: None,
                wheel_delta: None,
                pointer_id: Some(1),
            };
            for event in [
                pointer("pointerdown", 1),
                pointer("pointermove", 1),
                pointer("pointermove", 1),
                pointer("pointerup", 0),
            ] {
                dispatch_mouse_event(&context, event).unwrap();
            }
            assert_eq!(
                context.eval::<Vec<String>, _>("captureLog").unwrap(),
                [
                    "down:hit",
                    "set:true",
                    "got:true:false:false",
                    "move:capture",
                    "release:false",
                    "lost:true:false:false",
                    "move:hit",
                    "got:true:false:false",
                    "up:true",
                    "lost:true:false:false",
                ]
            );
            assert_eq!(
                context
                    .eval::<Vec<String>, _>(
                        r#"[
                            (() => {
                                try { captureTarget.setPointerCapture(2); return 'none'; }
                                catch (error) { return error.name; }
                            })(),
                            (() => {
                                try { captureTarget.releasePointerCapture(2); return 'none'; }
                                catch (error) { return error.name; }
                            })(),
                        ]"#,
                    )
                    .unwrap(),
                ["NotFoundError", "NotFoundError"]
            );
            dispatch_mouse_event(&context, pointer("pointerdown", 1)).unwrap();
            assert!(context
                .eval::<bool, _>(
                    "captureWindow.removeChild(captureTarget); \
                     captureTarget.hasPointerCapture(1) === false",
                )
                .unwrap());
            dispatch_mouse_event(&context, pointer("pointerup", 0)).unwrap();
            assert_ne!(hit_target, capture_target);
        });
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queued_click_bubbles_and_honors_dispatch_controls() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (plugin, state) = DomPlugin::new();
                let (runtime, driver) = runtime::Runtime::builder()
                    .plugin(plugin)
                    .build_driven()
                    .await
                    .unwrap();
                let driver = tokio::task::spawn_local(driver.run());

                runtime
                    .eval::<()>(
                        "globalThis.clickResult = [];\n\
                         globalThis.clickEvent = null;\n\
                         globalThis.immediateEvent = null;\n\
                         globalThis.clickWindow = app.createElement('window');\n\
                         globalThis.clickTarget = app.createElement('div');\n\
                         globalThis.immediateTarget = app.createElement('div');\n\
                         clickTarget.style.setProperty('width', '20px');\n\
                         clickTarget.style.setProperty('height', '10px');\n\
                         const removed = () => clickResult.push('removed');\n\
                         clickTarget.addEventListener('click', function (event) {\n\
                           globalThis.clickEvent = event;\n\
                           clickResult.push(event.type,\n\
                             String(event.target === clickTarget),\n\
                             String(event.currentTarget === clickTarget),\n\
                             String(this === clickTarget),\n\
                             String(event.clientX), String(event.clientY),\n\
                             String(event.button),\n\
                             String(event.target.getBoundingClientRect().width));\n\
                           event.preventDefault();\n\
                           clickTarget.removeEventListener('click', removed);\n\
                         });\n\
                         clickTarget.addEventListener('click', removed);\n\
                         clickTarget.addEventListener('click',\n\
                           () => clickResult.push('target-2'));\n\
                         clickTarget.addEventListener('click', () => {\n\
                           throw new Error('expected listener failure');\n\
                         });\n\
                         clickTarget.addEventListener('click',\n\
                           () => clickResult.push('after-error'));\n\
                         clickWindow.addEventListener('click', function (event) {\n\
                           clickResult.push(event.currentTarget === clickWindow\n\
                             && this === clickWindow ? 'window' : 'wrong-window');\n\
                           event.stopPropagation();\n\
                         });\n\
                         app.addEventListener('click', () => clickResult.push('app'));\n\
                         immediateTarget.addEventListener('click', event => {\n\
                           globalThis.immediateEvent = event;\n\
                           clickResult.push('immediate');\n\
                           event.stopImmediatePropagation();\n\
                         });\n\
                         immediateTarget.addEventListener('click',\n\
                           () => clickResult.push('skipped'));\n\
                         clickWindow.appendChild(clickTarget);\n\
                         clickWindow.appendChild(immediateTarget);\n\
                         app.appendChild(clickWindow);",
                    )
                    .await
                    .unwrap();
                let mut layout = LayoutEngine::new(TextEngine::without_system_fonts());
                let (target, immediate_target, presented_revision) = {
                    let state = state.borrow();
                    layout
                        .compute(&state.dom, LogicalViewport::new(320.0, 240.0).unwrap())
                        .unwrap();
                    let presented_revision = state.dom.revision();
                    state.publish_presented_layout(layout.current_shared().unwrap());
                    let window = state.dom.children(state.dom.root()).unwrap()[0];
                    let children = state.dom.children(window).unwrap();
                    (children[0], children[1], presented_revision)
                };
                runtime
                    .eval::<()>("clickTarget.style.setProperty('width', '30px')")
                    .await
                    .unwrap();
                assert!(state.borrow().dom.revision() > presented_revision);

                for (target, client_x, client_y) in
                    [(target, 12.5, 8.25), (immediate_target, 20.0, 10.0)]
                {
                    for (pressed, buttons) in [
                        (true, PressedMouseButtons::from_bits(1)),
                        (false, PressedMouseButtons::NONE),
                    ] {
                        state
                            .borrow()
                            .enqueue_mouse_input(NativeMouseInput {
                                kind: NativeMouseInputKind::Button {
                                    button: ChangedMouseButton::PRIMARY,
                                    pressed,
                                },
                                hit_target: Some(target),
                                presented_revision,
                                client_x,
                                client_y,
                                buttons,
                            })
                            .unwrap();
                    }
                }
                let result: Vec<String> = runtime
                    .eval(
                        "[...clickResult,\n\
                         String(clickEvent.defaultPrevented),\n\
                         String(clickEvent.currentTarget === null),\n\
                         String(clickEvent.bubbles),\n\
                         String(clickEvent.cancelable),\n\
                         String(immediateEvent.currentTarget === null)]",
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    result,
                    [
                        "click",
                        "true",
                        "true",
                        "true",
                        "12.5",
                        "8.25",
                        "0",
                        "20",
                        "target-2",
                        "after-error",
                        "window",
                        "immediate",
                        "true",
                        "true",
                        "true",
                        "true",
                        "true",
                    ]
                );

                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }
}
