//! JavaScript pointer-event construction and listener execution.

use rquickjs::{Class, Ctx, Object, Result as JsResult};

use super::super::{
    classes::{borrow, borrow_mut, NativeNode},
    errors,
};
use super::mouse::{execute_mouse_event, execute_pointing_event, PointingEvent};
use crate::ui::{
    events::{DomPointerEvent, PointerEventKind},
    host::NativeMouseInput,
};

pub(super) fn execute_mouse_input(context: &Ctx<'_>, input: NativeMouseInput) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let plan = {
        let mut state = borrow_mut(context, &state)?;
        let state = &mut *state;
        state.pointer.plan_input(&state.dom, input)
    };
    for event in plan.boundaries {
        execute_pointer_event(context, event)?;
    }
    if let Some(event) = plan.pointer {
        execute_pointer_event(context, event)?;
    }
    if let Some(event) = plan.mouse {
        execute_mouse_event(context, event)?;
    }
    Ok(())
}
pub(super) fn execute_pointer_cancel(context: &Ctx<'_>) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let Some(event) = borrow(context, &state)?.pointer.cancel_event() else {
        return Ok(());
    };
    execute_pointer_event(context, event)
}

fn js_event_type(kind: PointerEventKind) -> &'static str {
    match kind {
        PointerEventKind::Down => "pointerdown",
        PointerEventKind::Up => "pointerup",
        PointerEventKind::Move => "pointermove",
        PointerEventKind::Enter => "pointerenter",
        PointerEventKind::Leave => "pointerleave",
        PointerEventKind::Cancel => "pointercancel",
        PointerEventKind::GotCapture => "gotpointercapture",
        PointerEventKind::LostCapture => "lostpointercapture",
    }
}

fn execute_pointer_event_inner(context: &Ctx<'_>, pointer: DomPointerEvent) -> JsResult<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let plan = {
        let state = borrow(context, &state)?;
        errors::map_dom(
            context,
            "plan pointer dispatch",
            pointer.plan_dispatch(&state.dom),
        )?
    };
    let Some(plan) = plan else {
        return Ok(());
    };
    let pointer = plan.event;
    execute_pointing_event(
        context,
        &state,
        js_event_type(pointer.kind),
        PointingEvent {
            path: plan.path,
            bubbles: plan.bubbles,
            cancelable: plan.cancelable,
            client_x: pointer.client_x,
            client_y: pointer.client_y,
            button: pointer.button,
            buttons: pointer.buttons,
            related_target: pointer.related_target,
            wheel_delta: None,
            pointer_id: pointer.pointer_id,
        },
    )
}

pub(in crate::ui::dom_plugin) fn execute_pointer_event(
    context: &Ctx<'_>,
    pointer: DomPointerEvent,
) -> JsResult<()> {
    if !pointer.uses_pointer_state() {
        return execute_pointer_event_inner(context, pointer);
    }

    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let (transitions, pointer) = {
        let mut state = borrow_mut(context, &state)?;
        let state = &mut *state;
        state.pointer.start_dispatch(&state.dom, pointer)
    };
    for event in transitions {
        execute_pointer_event_inner(context, event)?;
    }

    let result = execute_pointer_event_inner(context, pointer);
    let transitions = {
        let mut state = borrow_mut(context, &state)?;
        let state = &mut *state;
        state.pointer.finish_dispatch(&state.dom, pointer)
    };
    result?;
    for event in transitions {
        execute_pointer_event_inner(context, event)?;
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
        host::{ChangedMouseButton, NativeMouseInputKind, PressedMouseButtons},
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
            let pointer = |kind, buttons| DomPointerEvent {
                kind,
                target: hit_target,
                presented_revision: revision,
                client_x: 10.0,
                client_y: 20.0,
                button: ChangedMouseButton::PRIMARY,
                buttons: PressedMouseButtons::from_bits(buttons),
                related_target: None,
                pointer_id: 1,
            };
            for event in [
                pointer(PointerEventKind::Down, 1),
                pointer(PointerEventKind::Move, 1),
                pointer(PointerEventKind::Move, 1),
                pointer(PointerEventKind::Up, 0),
            ] {
                execute_pointer_event(&context, event).unwrap();
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
            execute_pointer_event(&context, pointer(PointerEventKind::Down, 1)).unwrap();
            assert!(context
                .eval::<bool, _>(
                    "captureWindow.removeChild(captureTarget); \
                     captureTarget.hasPointerCapture(1) === false",
                )
                .unwrap());
            execute_pointer_event(&context, pointer(PointerEventKind::Up, 0)).unwrap();
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
