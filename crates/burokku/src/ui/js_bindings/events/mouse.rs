//! JavaScript mouse/wheel-event construction and listener execution.

use rquickjs::{
    class::Trace, prelude::This, CatchResultExt, Class, Ctx, IntoJs, JsLifetime, Null, Object,
    Result as JsResult, Value,
};

use super::super::{borrow, errors, node::NativeNode, wrapper::wrap_node, SharedDomBindings};
use crate::ui::events::{DomMouseEvent, MouseEventKind};

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

pub(super) struct PointingEvent {
    pub(super) path: Vec<crate::ui::elements::NodeId>,
    pub(super) bubbles: bool,
    pub(super) cancelable: bool,
    pub(super) client_x: f64,
    pub(super) client_y: f64,
    pub(super) button: crate::ui::host::ChangedMouseButton,
    pub(super) buttons: crate::ui::host::PressedMouseButtons,
    pub(super) related_target: Option<crate::ui::elements::NodeId>,
    pub(super) wheel_delta: Option<(f64, f64, crate::ui::host::WheelDeltaMode)>,
    pub(super) pointer_id: u32,
}
fn js_event_type(kind: MouseEventKind) -> &'static str {
    match kind {
        MouseEventKind::Click => "click",
        MouseEventKind::Wheel => "wheel",
    }
}

pub(in crate::ui::js_bindings) fn execute_mouse_event(
    context: &Ctx<'_>,
    mouse: DomMouseEvent,
) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let plan = {
        let state = borrow(context, &state)?;
        errors::map_dom(
            context,
            "plan mouse dispatch",
            mouse.plan_dispatch(&state.dom),
        )?
    };
    let Some(plan) = plan else {
        return Ok(());
    };
    let mouse = plan.event;
    execute_pointing_event(
        context,
        &state,
        js_event_type(mouse.kind),
        PointingEvent {
            path: plan.path,
            bubbles: plan.bubbles,
            cancelable: plan.cancelable,
            client_x: mouse.client_x,
            client_y: mouse.client_y,
            button: mouse.button,
            buttons: mouse.buttons,
            related_target: None,
            wheel_delta: mouse.wheel_delta,
            pointer_id: 0,
        },
    )
}

pub(super) fn execute_pointing_event(
    context: &Ctx<'_>,
    state: &SharedDomBindings,
    event_type: &'static str,
    mouse: PointingEvent,
) -> JsResult<()> {
    let mut listeners = Vec::with_capacity(mouse.path.len());
    for id in mouse.path {
        let current = wrap_node(context, state, id)?;
        let node =
            Class::<NativeNode>::from_object(&current).expect("wrapped nodes use NativeNode");
        let callbacks = node.borrow().listeners.matching(event_type);
        listeners.push((current, callbacks));
    }
    if listeners.iter().all(|(_, callbacks)| callbacks.is_empty()) {
        return Ok(());
    }

    let target = listeners[0].0.clone();
    let related_target = mouse
        .related_target
        .map(|id| wrap_node(context, state, id))
        .transpose()?;
    let (delta_x, delta_y, delta_mode) = mouse
        .wheel_delta
        .map(|(delta_x, delta_y, mode)| (delta_x, delta_y, mode.code()))
        .unwrap_or((0.0, 0.0, 0));
    let pointer_id = mouse.pointer_id;
    let event = Class::instance(
        context.clone(),
        MouseEvent {
            event_type: event_type.into(),
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
            bubbles: mouse.bubbles,
            cancelable: mouse.cancelable,
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
            let still_registered = node.borrow().listeners.contains(event_type, listener.id);
            if !still_registered {
                continue;
            }
            if let Err(error) = listener
                .callback
                .call::<_, ()>((This(current.clone()), event.clone()))
                .catch(context)
            {
                eprintln!("Burokku {} listener failed: {error}", event_type);
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

    use super::*;
    use crate::plugins::dom::DomPlugin;
    use crate::ui::host::{ChangedMouseButton, PressedMouseButtons, WheelDeltaMode};

    fn context() -> (JsRuntime, Context) {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        (runtime, context)
    }

    #[test]
    fn mouse_dispatch_preserves_payload_and_propagation() {
        let (plugin, _) = DomPlugin::new_with_bindings();
        let (_runtime, context) = context();
        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    r#"
                globalThis.mouseWindow = app.createElement('window');
                globalThis.mouseTarget = app.createElement('div');
                mouseWindow.appendChild(mouseTarget);
                app.appendChild(mouseWindow);
                globalThis.mouseCalls = [];
                globalThis.mouseChecks = [];
                globalThis.lastMouseEvent = null;
                for (const type of ['click', 'wheel']) {
                    mouseTarget.addEventListener(type, function (event) {
                        lastMouseEvent = event;
                        mouseCalls.push('target');
                        mouseChecks.push(event.type === type,
                            event.target === mouseTarget,
                            event.currentTarget === mouseTarget, this === mouseTarget,
                            event.clientX === 12.5, event.clientY === 8.25,
                            event.button === 2, event.buttons === 3,
                            event.relatedTarget === null, event.pointerId === 0,
                            type !== 'wheel' || (event.deltaX === 4.5 &&
                                event.deltaY === -6.25 && event.deltaMode === 1));
                        event.preventDefault();
                    });
                    mouseWindow.addEventListener(type, () => mouseCalls.push('window'));
                    app.addEventListener(type, () => mouseCalls.push('app'));
                }
            "#,
                )
                .unwrap();
            let (target, presented_revision) = {
                let state = plugin.state();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                (state.dom.children(window).unwrap()[0], state.dom.revision())
            };
            for kind in [MouseEventKind::Click, MouseEventKind::Wheel] {
                let event_type = js_event_type(kind);
                context
                    .eval::<(), _>("mouseCalls = []; mouseChecks = []")
                    .unwrap();
                let event = DomMouseEvent {
                    kind,
                    target,
                    presented_revision,
                    client_x: 12.5,
                    client_y: 8.25,
                    button: ChangedMouseButton::SECONDARY,
                    buttons: PressedMouseButtons::from_bits(3),
                    wheel_delta: (kind == MouseEventKind::Wheel).then_some((
                        4.5,
                        -6.25,
                        WheelDeltaMode::Line,
                    )),
                };
                execute_mouse_event(&context, event).unwrap();
                assert_eq!(
                    context.eval::<Vec<String>, _>("mouseCalls").unwrap(),
                    ["target", "window", "app"],
                    "{event_type}"
                );
                assert!(
                    context
                        .eval::<bool, _>("mouseChecks.every(Boolean)")
                        .unwrap(),
                    "{event_type}"
                );
                let flags: Vec<bool> = context
                    .eval(
                        "[lastMouseEvent.bubbles, lastMouseEvent.cancelable, \
                         lastMouseEvent.defaultPrevented, lastMouseEvent.currentTarget === null]",
                    )
                    .unwrap();
                assert_eq!(flags, [true, true, true, true], "{event_type}");

                context
                    .eval::<(), _>("mouseWindow.removeChild(mouseTarget); mouseCalls = []")
                    .unwrap();
                execute_mouse_event(&context, event).unwrap();
                assert!(context.eval::<bool, _>("mouseCalls.length === 0").unwrap());
                context
                    .eval::<(), _>("mouseWindow.appendChild(mouseTarget)")
                    .unwrap();
            }
        });
    }
}
