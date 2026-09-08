//! DOM keyboard event payloads and listener propagation.

use super::super::{
    classes::{borrow, wrap_node, NativeNode},
    errors,
};
use crate::ui::host::NativeKeyboardEvent;
use rquickjs::{
    class::Trace, prelude::This, CatchResultExt, Class, Ctx, IntoJs, JsLifetime, Null, Object,
    Result as JsResult, Value,
};

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "BurokkuKeyboardEvent", rename_all = "camelCase")]
struct KeyboardEvent<'js> {
    #[qjs(get, enumerable, rename = "type")]
    event_type: String,
    #[qjs(get, enumerable)]
    target: Object<'js>,
    current_target: Option<Object<'js>>,
    #[qjs(get, enumerable)]
    key: String,
    #[qjs(get, enumerable)]
    key_code: u16,
    #[qjs(get, enumerable)]
    repeat: bool,
    #[qjs(get, enumerable)]
    shift_key: bool,
    #[qjs(get, enumerable)]
    ctrl_key: bool,
    #[qjs(get, enumerable)]
    alt_key: bool,
    #[qjs(get, enumerable)]
    meta_key: bool,
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
impl<'js> KeyboardEvent<'js> {
    #[qjs(get, rename = "currentTarget", enumerable)]
    fn current_target(&self, context: Ctx<'js>) -> JsResult<Value<'js>> {
        match &self.current_target {
            Some(target) => Ok(target.clone().into_value()),
            None => Null.into_js(&context),
        }
    }

    #[qjs(rename = "preventDefault")]
    fn prevent_default(&mut self) {
        self.default_prevented = true;
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

pub(super) fn dispatch_keyboard_event(
    context: &Ctx<'_>,
    keyboard: NativeKeyboardEvent,
) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let path = {
        let state = borrow(context, &state)?;
        if !state.dom.is_connected(keyboard.target).unwrap_or(false) {
            return Ok(());
        }
        let mut path = Vec::new();
        let mut current = Some(keyboard.target);
        while let Some(id) = current {
            path.push(id);
            current = errors::map_dom(
                context,
                "build keyboard propagation path",
                state.dom.parent_node(id),
            )?;
        }
        path
    };

    let mut listeners = Vec::with_capacity(path.len());
    for id in path {
        let current = wrap_node(context, &state, id)?;
        let node =
            Class::<NativeNode>::from_object(&current).expect("wrapped nodes use NativeNode");
        let callbacks = node
            .borrow()
            .listeners
            .get(keyboard.event_type)
            .cloned()
            .unwrap_or_default();
        listeners.push((current, callbacks));
    }
    if listeners.iter().all(|(_, callbacks)| callbacks.is_empty()) {
        return Ok(());
    }

    let target = listeners[0].0.clone();
    let event = Class::instance(
        context.clone(),
        KeyboardEvent {
            event_type: keyboard.event_type.into(),
            target,
            current_target: None,
            key: keyboard.key,
            key_code: keyboard.key_code,
            repeat: keyboard.repeat,
            shift_key: keyboard.modifiers.shift,
            ctrl_key: keyboard.modifiers.control,
            alt_key: keyboard.modifiers.alt,
            meta_key: keyboard.modifiers.command,
            bubbles: true,
            cancelable: true,
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
                .get(keyboard.event_type)
                .is_some_and(|listeners| listeners.iter().any(|item| item.id == listener.id));
            if !still_registered {
                continue;
            }
            if let Err(error) = listener
                .callback
                .call::<_, ()>((This(current.clone()), event.clone()))
                .catch(context)
            {
                eprintln!("Burokku {} listener failed: {error}", keyboard.event_type);
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
    use winit::Modifiers;

    use super::*;
    use crate::ui::dom_plugin::DomPlugin;

    fn context() -> (JsRuntime, Context) {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        (runtime, context)
    }

    #[test]
    fn keyboard_dispatch_preserves_payload_and_bubbles() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();
        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    r#"
                globalThis.keyWindow = app.createElement('window');
                app.appendChild(keyWindow);
                globalThis.keyCalls = [];
                globalThis.keyChecks = [];
                globalThis.lastKeyEvent = null;
                for (const type of ['keydown', 'keyup']) {
                    keyWindow.addEventListener(type, function (event) {
                        lastKeyEvent = event;
                        keyCalls.push('window');
                        keyChecks.push(event.type === type, event.target === keyWindow,
                            event.currentTarget === keyWindow, this === keyWindow,
                            event.key === 'A', event.keyCode === 0,
                            event.repeat === (type === 'keydown'),
                            event.shiftKey, event.ctrlKey, !event.altKey, event.metaKey,
                            event.bubbles, event.cancelable);
                        try { event.key = 'B'; } catch {}
                        keyChecks.push(event.key === 'A');
                        event.preventDefault();
                    });
                    app.addEventListener(type, event => {
                        keyCalls.push('app');
                        keyChecks.push(event.currentTarget === app);
                    });
                }
            "#,
                )
                .unwrap();
            let target = plugin
                .state()
                .dom
                .children(plugin.state().dom.root())
                .unwrap()[0];
            let modifiers = Modifiers {
                shift: true,
                control: true,
                command: true,
                ..Modifiers::default()
            };
            for event_type in ["keydown", "keyup"] {
                context
                    .eval::<(), _>("keyCalls = []; keyChecks = []")
                    .unwrap();
                dispatch_keyboard_event(
                    &context,
                    NativeKeyboardEvent {
                        event_type,
                        target,
                        key: "A".into(),
                        key_code: 0,
                        repeat: event_type == "keydown",
                        modifiers,
                    },
                )
                .unwrap();
                assert_eq!(
                    context.eval::<Vec<String>, _>("keyCalls").unwrap(),
                    ["window", "app"]
                );
                assert!(context.eval::<bool, _>("keyChecks.every(Boolean)").unwrap());
                let flags: Vec<bool> = context
                    .eval("[lastKeyEvent.defaultPrevented, lastKeyEvent.currentTarget === null]")
                    .unwrap();
                assert_eq!(flags, [true, true]);
            }
        });
    }
}
