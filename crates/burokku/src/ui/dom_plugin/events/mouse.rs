//! DOM mouse/wheel event payloads and listener propagation.

use rquickjs::{
    class::Trace, prelude::This, CatchResultExt, Class, Ctx, IntoJs, JsLifetime, Null, Object,
    Result as JsResult, Value,
};

use super::super::{
    classes::{borrow, wrap_node, NativeNode},
    errors,
};
use crate::ui::{
    elements::NodeId,
    host::{ChangedMouseButton, PressedMouseButtons, WheelDeltaMode},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui::dom_plugin) struct NativeMouseEvent {
    pub(in crate::ui::dom_plugin) event_type: &'static str,
    pub(in crate::ui::dom_plugin) target: NodeId,
    pub(in crate::ui::dom_plugin) presented_revision: u64,
    pub(in crate::ui::dom_plugin) client_x: f64,
    pub(in crate::ui::dom_plugin) client_y: f64,
    pub(in crate::ui::dom_plugin) button: ChangedMouseButton,
    pub(in crate::ui::dom_plugin) buttons: PressedMouseButtons,
    pub(in crate::ui::dom_plugin) related_target: Option<NodeId>,
    pub(in crate::ui::dom_plugin) wheel_delta: Option<(f64, f64, WheelDeltaMode)>,
    pub(in crate::ui::dom_plugin) pointer_id: Option<u32>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "BurokkuMouseEvent", rename_all = "camelCase")]
struct MouseEvent<'js> {
    #[qjs(get, enumerable, rename = "type")]
    event_type: String,
    #[qjs(get, enumerable)]
    target: Object<'js>,
    current_target: Option<Object<'js>>,
    #[qjs(get, enumerable)]
    client_x: f64,
    #[qjs(get, enumerable)]
    client_y: f64,
    #[qjs(get, enumerable)]
    button: i32,
    #[qjs(get, enumerable)]
    buttons: u16,
    #[qjs(get, enumerable)]
    delta_x: f64,
    #[qjs(get, enumerable)]
    delta_y: f64,
    #[qjs(get, enumerable)]
    delta_mode: u16,
    #[qjs(get, enumerable)]
    pointer_id: u32,
    #[qjs(get, enumerable)]
    pointer_type: String,
    #[qjs(get, enumerable)]
    is_primary: bool,
    related_target: Option<Object<'js>>,
    #[qjs(get, enumerable)]
    bubbles: bool,
    #[qjs(get, enumerable)]
    cancelable: bool,
    #[qjs(get, enumerable)]
    default_prevented: bool,
    propagation_stopped: bool,
    immediate_propagation_stopped: bool,
}

#[rquickjs::methods]
impl<'js> MouseEvent<'js> {
    #[qjs(get, rename = "relatedTarget", enumerable)]
    fn related_target(&self, context: Ctx<'js>) -> JsResult<Value<'js>> {
        match &self.related_target {
            Some(target) => Ok(target.clone().into_value()),
            None => Null.into_js(&context),
        }
    }

    #[qjs(get, rename = "currentTarget", enumerable)]
    fn current_target(&self, context: Ctx<'js>) -> JsResult<Value<'js>> {
        match &self.current_target {
            Some(target) => Ok(target.clone().into_value()),
            None => Null.into_js(&context),
        }
    }

    #[qjs(rename = "preventDefault")]
    fn prevent_default(&mut self) {
        if self.cancelable {
            self.default_prevented = true;
        }
    }

    #[qjs(rename = "stopPropagation")]
    fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    #[qjs(rename = "stopImmediatePropagation")]
    fn stop_immediate_propagation(&mut self) {
        self.propagation_stopped = true;
        self.immediate_propagation_stopped = true;
    }
}

pub(super) fn dispatch_mouse_event_inner(
    context: &Ctx<'_>,
    mouse: NativeMouseEvent,
) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let bubbles = !matches!(mouse.event_type, "pointerenter" | "pointerleave");
    let (path, related_target) = {
        let state = borrow(context, &state)?;
        debug_assert!(mouse.presented_revision <= state.dom.revision());
        if !state.dom.is_connected(mouse.target).unwrap_or(false) {
            return Ok(());
        }

        let mut path = Vec::new();
        let mut current = Some(mouse.target);
        while let Some(id) = current {
            path.push(id);
            if !bubbles {
                break;
            }
            current = errors::map_dom(
                context,
                "build mouse propagation path",
                state.dom.parent_node(id),
            )?;
        }
        (
            path,
            mouse
                .related_target
                .filter(|id| state.dom.node(*id).is_some()),
        )
    };

    let mut listeners = Vec::with_capacity(path.len());
    for id in path {
        let current = wrap_node(context, &state, id)?;
        let node =
            Class::<NativeNode>::from_object(&current).expect("wrapped nodes use NativeNode");
        let callbacks = node
            .borrow()
            .listeners
            .get(mouse.event_type)
            .cloned()
            .unwrap_or_default();
        listeners.push((current, callbacks));
    }
    if listeners.iter().all(|(_, callbacks)| callbacks.is_empty()) {
        return Ok(());
    }

    let target = listeners[0].0.clone();
    let related_target = related_target
        .map(|id| wrap_node(context, &state, id))
        .transpose()?;
    let (delta_x, delta_y, delta_mode) = mouse
        .wheel_delta
        .map(|(delta_x, delta_y, mode)| (delta_x, delta_y, mode.code()))
        .unwrap_or((0.0, 0.0, 0));
    let pointer_id = mouse.pointer_id.unwrap_or(0);
    let cancelable = bubbles
        && !matches!(
            mouse.event_type,
            "pointercancel" | "gotpointercapture" | "lostpointercapture"
        );
    let event = Class::instance(
        context.clone(),
        MouseEvent {
            event_type: mouse.event_type.into(),
            target,
            current_target: None,
            client_x: mouse.client_x,
            client_y: mouse.client_y,
            button: mouse.button.code(),
            buttons: mouse.buttons.bits(),
            delta_x,
            delta_y,
            delta_mode,
            pointer_id,
            pointer_type: if pointer_id == 0 { "" } else { "mouse" }.into(),
            is_primary: pointer_id != 0,
            related_target,
            bubbles,
            cancelable,
            default_prevented: false,
            propagation_stopped: false,
            immediate_propagation_stopped: false,
        },
    )?;

    for (current, callbacks) in listeners {
        event.borrow_mut().current_target = Some(current.clone());
        for listener in callbacks {
            let node =
                Class::<NativeNode>::from_object(&current).expect("wrapped nodes use NativeNode");
            let still_registered = node
                .borrow()
                .listeners
                .get(mouse.event_type)
                .is_some_and(|listeners| listeners.iter().any(|item| item.id == listener.id));
            if !still_registered {
                continue;
            }
            if let Err(error) = listener
                .callback
                .call::<_, ()>((This(current.clone()), event.clone()))
                .catch(context)
            {
                eprintln!("Burokku {} listener failed: {error}", mouse.event_type);
            }
            if event.borrow().immediate_propagation_stopped {
                break;
            }
        }
        if event.borrow().propagation_stopped {
            break;
        }
    }
    event.borrow_mut().current_target = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use runtime::{
        rquickjs::{Context, Runtime as JsRuntime},
        Plugin,
    };

    use super::{super::pointer::dispatch_mouse_event, *};
    use crate::ui::dom_plugin::DomPlugin;

    fn context() -> (JsRuntime, Context) {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        (runtime, context)
    }

    #[test]
    fn pointing_dispatch_preserves_payload_and_propagation() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();
        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    r#"
                globalThis.mouseWindow = app.createElement('window');
                globalThis.mouseTarget = app.createElement('div');
                globalThis.mouseRelated = app.createElement('div');
                mouseWindow.appendChild(mouseTarget);
                mouseWindow.appendChild(mouseRelated);
                app.appendChild(mouseWindow);
                globalThis.mouseCalls = [];
                globalThis.mouseChecks = [];
                globalThis.lastMouseEvent = null;
                for (const type of ['click', 'wheel', 'pointerdown', 'pointerup', 'pointermove',
                                    'pointerenter', 'pointerleave', 'pointercancel']) {
                    mouseTarget.addEventListener(type, function (event) {
                        lastMouseEvent = event;
                        mouseCalls.push('target');
                        mouseChecks.push(event.type === type,
                            event.target === mouseTarget,
                            event.currentTarget === mouseTarget, this === mouseTarget,
                            event.clientX === 12.5, event.clientY === 8.25,
                            event.button === 2, event.buttons === 3,
                            event.relatedTarget === mouseRelated,
                            type !== 'wheel' || (event.deltaX === 4.5 &&
                                event.deltaY === -6.25 && event.deltaMode === 1),
                            !type.startsWith('pointer') || (event.pointerId === 1 &&
                                event.pointerType === 'mouse' && event.isPrimary));
                        try { event.buttons = 99; } catch {}
                        try { event.relatedTarget = mouseTarget; } catch {}
                        try { event.deltaY = 99; } catch {}
                        try { event.pointerId = 2; } catch {}
                        mouseChecks.push(event.buttons === 3,
                            event.relatedTarget === mouseRelated,
                            type !== 'wheel' || event.deltaY === -6.25,
                            !type.startsWith('pointer') || event.pointerId === 1);
                        event.preventDefault();
                    });
                    mouseWindow.addEventListener(type, function (event) {
                        mouseCalls.push('window');
                        mouseChecks.push(event.currentTarget === mouseWindow,
                            this === mouseWindow, event.target === mouseTarget);
                    });
                    app.addEventListener(type, () => mouseCalls.push('app'));
                }
            "#,
                )
                .unwrap();
            let (target, related_target, presented_revision) = {
                let state = plugin.state();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                let children = state.dom.children(window).unwrap();
                (children[0], children[1], state.dom.revision())
            };
            for event_type in [
                "click",
                "wheel",
                "pointerdown",
                "pointerup",
                "pointermove",
                "pointerenter",
                "pointerleave",
                "pointercancel",
            ] {
                context
                    .eval::<(), _>("mouseCalls = []; mouseChecks = []")
                    .unwrap();
                dispatch_mouse_event(
                    &context,
                    NativeMouseEvent {
                        event_type,
                        target,
                        presented_revision,
                        client_x: 12.5,
                        client_y: 8.25,
                        button: ChangedMouseButton::SECONDARY,
                        buttons: PressedMouseButtons::from_bits(3),
                        related_target: Some(related_target),
                        wheel_delta: (event_type == "wheel").then_some((
                            4.5,
                            -6.25,
                            WheelDeltaMode::Line,
                        )),
                        pointer_id: event_type.starts_with("pointer").then_some(1),
                    },
                )
                .unwrap();
                let bubbles = !matches!(event_type, "pointerenter" | "pointerleave");
                let cancelable = bubbles && event_type != "pointercancel";
                let calls: Vec<String> = context.eval("mouseCalls").unwrap();
                let expected = if bubbles {
                    vec!["target", "window", "app"]
                } else {
                    vec!["target"]
                };
                assert_eq!(calls, expected, "{event_type}");
                assert!(
                    context
                        .eval::<bool, _>("mouseChecks.every(Boolean)")
                        .unwrap(),
                    "{event_type}"
                );
                let flags: Vec<bool> = context
                    .eval(
                        "[lastMouseEvent.bubbles, lastMouseEvent.cancelable,
                      lastMouseEvent.defaultPrevented, lastMouseEvent.currentTarget === null]",
                    )
                    .unwrap();
                assert_eq!(
                    flags,
                    [bubbles, cancelable, cancelable, true],
                    "{event_type}"
                );
            }
            // Leaving the window has no related node.
            context
                .eval::<(), _>("mouseRelated = null; mouseChecks = []")
                .unwrap();
            let leave = NativeMouseEvent {
                event_type: "pointerleave",
                target,
                presented_revision,
                client_x: 12.5,
                client_y: 8.25,
                button: ChangedMouseButton::SECONDARY,
                buttons: PressedMouseButtons::from_bits(3),
                related_target: None,
                wheel_delta: None,
                pointer_id: Some(1),
            };
            dispatch_mouse_event(&context, leave).unwrap();
            assert!(context
                .eval::<bool, _>("mouseChecks.every(Boolean)")
                .unwrap());

            // A queued event must not reach a target detached before dispatch.
            context
                .eval::<(), _>("mouseWindow.removeChild(mouseTarget); mouseCalls = []")
                .unwrap();
            dispatch_mouse_event(&context, leave).unwrap();
            assert!(context.eval::<bool, _>("mouseCalls.length === 0").unwrap());
        });
    }
}
