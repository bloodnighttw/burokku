//! UI-thread ownership and QuickJS bindings for the live DOM.

use std::{cell::RefCell, rc::Rc};

use runtime::{rquickjs::Ctx, JsTaskQueue, JsTaskQueueError, Plugin};

use super::{
    elements::{Dom, DomError, NodeId, ReclaimReport},
    layout::ComputedLayout,
};

mod classes;
mod errors;
mod lifetime;

use lifetime::SharedWrapperRoots;

pub(crate) type SharedUiDom = Rc<RefCell<UiDomState>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeMouseEvent {
    pub(crate) event_type: &'static str,
    pub(crate) target: NodeId,
    pub(crate) presented_revision: u64,
    pub(crate) client_x: f64,
    pub(crate) client_y: f64,
    pub(crate) button: u16,
    pub(crate) buttons: u16,
    pub(crate) related_target: Option<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug)]
pub(crate) struct UiDomState {
    pub(crate) dom: Dom,
    wrapper_roots: SharedWrapperRoots,
    pub(crate) last_reclaim: ReclaimReport,
    task_queue: Option<JsTaskQueue>,
    presented_layout: RefCell<Option<Rc<ComputedLayout>>>,
}

impl UiDomState {
    pub(crate) fn publish_presented_layout(&self, computed: Rc<ComputedLayout>) {
        self.presented_layout.replace(Some(computed));
    }

    pub(crate) fn enqueue_mouse_events(
        &self,
        events: Vec<NativeMouseEvent>,
    ) -> Result<(), JsTaskQueueError> {
        self.task_queue
            .as_ref()
            .ok_or(JsTaskQueueError::Closed)?
            .try_enqueue(move |context| {
                for event in events {
                    classes::dispatch_mouse_event(context, event)?;
                }
                Ok(())
            })
    }

    pub(crate) fn layout_rect(&self, id: NodeId) -> Result<Option<LayoutRect>, DomError> {
        self.dom.element_tag(id)?;
        let layout = self.presented_layout.borrow();
        Ok(layout
            .as_deref()
            .and_then(|layout| layout.box_for(id))
            .map(|computed_box| {
                let origin = computed_box.border_origin();
                let size = computed_box.layout().size;
                LayoutRect {
                    x: origin.x,
                    y: origin.y,
                    width: size.width,
                    height: size.height,
                }
            }))
    }
}

/// Installs bindings backed by the UI thread's live DOM.
pub(crate) struct DomPlugin {
    state: SharedUiDom,
}

impl DomPlugin {
    pub(crate) fn new() -> (Self, SharedUiDom) {
        let state = Rc::new(RefCell::new(UiDomState {
            dom: Dom::new(),
            wrapper_roots: SharedWrapperRoots::default(),
            last_reclaim: ReclaimReport::default(),
            task_queue: None,
            presented_layout: RefCell::new(None),
        }));
        (
            Self {
                state: Rc::clone(&state),
            },
            state,
        )
    }

    #[cfg(test)]
    #[cfg(test)]
    fn reclaim_for_test(&self) {
        self.state
            .try_borrow_mut()
            .expect("DOM plugin state is not borrowed")
            .reclaim_detached()
            .unwrap();
    }

    #[cfg(test)]
    fn state(&self) -> std::cell::Ref<'_, UiDomState> {
        self.state
            .try_borrow()
            .expect("DOM plugin state is not borrowed")
    }
}

impl Plugin for DomPlugin {
    fn name(&self) -> &'static str {
        "burokku-dom"
    }

    fn install<'js>(&self, context: &Ctx<'js>) -> runtime::Result<()> {
        self.state
            .try_borrow_mut()
            .map_err(|_| runtime::rquickjs::Error::Unknown)?
            .task_queue = JsTaskQueue::from_context(context).ok();
        classes::install(context, Rc::clone(&self.state))
    }
}

#[cfg(test)]
mod tests {

    use runtime::rquickjs::{CatchResultExt, Context, Object, Runtime as JsRuntime};

    use super::*;
    use crate::ui::{
        elements::{Element, NodeKind},
        layout::{LayoutEngine, LogicalViewport},
        text::TextEngine,
    };

    fn context() -> (JsRuntime, Context) {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        (runtime, context)
    }

    fn collect_garbage(runtime: &JsRuntime, context: &Context) {
        for _ in 0..3 {
            runtime.run_gc();
            context.with(|context| while context.execute_pending_job() {});
        }
    }

    fn subtree_text(dom: &Dom, root: NodeId) -> String {
        let mut text = String::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if let Some(value) = dom.text(id) {
                text.push_str(value);
            } else if let Some(children) = dom.children(id) {
                pending.extend(children.iter().rev().copied());
            }
        }
        text
    }

    #[test]
    fn installs_permanent_app_and_host_only_node_classes() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            let app: Object = context.globals().get("app").unwrap();
            assert!(app.instance_of::<classes::NativeNode>());
            let values: Vec<bool> = context
                .eval(
                    "[\
                        app instanceof AppNode,\
                        app instanceof Node,\
                        !(app instanceof Element),\
                        Object.getOwnPropertyDescriptor(globalThis, 'app').writable === false,\
                        (() => { try { new Node(); return false } catch (e) { return e instanceof TypeError } })()\
                    ]",
                )
                .unwrap();
            assert_eq!(values, [true, true, true, true, true]);
        });

        let state = plugin.state();
        assert_eq!(state.live_wrapper_count(), 1);
        assert!(state.has_wrapper(state.dom.root()));
    }

    #[test]
    fn facade_mutates_live_dom_synchronously() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            let values: Vec<bool> = context
                .eval(
                    "globalThis.kept = {};\
                     kept.window = app.createElement('window');\
                     kept.div = app.createElement('div');\
                     kept.paragraph = app.createElement('text');\
                     kept.text = app.createTextNode('before');\
                     kept.div.setAttribute('role', 'status');\
                     kept.div.style.setProperty('width', '20px');\
                     kept.paragraph.appendChild(kept.text);\
                     kept.div.appendChild(kept.paragraph);\
                     kept.window.appendChild(kept.div);\
                     app.appendChild(kept.window);\
                     kept.text.data = 'after';\
                     [\
                       app.firstChild === kept.window,\
                       kept.window.firstChild === kept.div,\
                       kept.div.firstChild === kept.paragraph,\
                       kept.paragraph.firstChild === kept.text,\
                       kept.text.parentNode === kept.paragraph,\
                       kept.text.textContent === 'after',\
                       kept.div.getAttribute('role') === 'status',\
                       kept.div.style === kept.div.style,\
                       kept.text.isConnected\
                     ]",
                )
                .unwrap();
            assert_eq!(
                values,
                [true, true, true, true, true, true, true, true, true]
            );
        });

        plugin.reclaim_for_test();
        let state = plugin.state();
        let dom = &state.dom;
        let window = dom.children(dom.root()).unwrap()[0];
        let div = dom.children(window).unwrap()[0];
        let paragraph = dom.children(div).unwrap()[0];
        let text = dom.children(paragraph).unwrap()[0];
        assert!(matches!(
            dom.kind(window),
            Some(NodeKind::Element(Element::Window { .. }))
        ));
        assert!(matches!(
            dom.kind(div),
            Some(NodeKind::Element(Element::Div { .. }))
        ));
        assert_eq!(dom.text(text), Some("after"));
        assert_eq!(dom.attribute(div, "role"), Some("status"));
    }

    #[test]
    fn element_exposes_last_presented_read_only_layout_rects() {
        let (plugin, state) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            assert!(context
                .eval::<bool, _>(
                    "globalThis.layoutWindow = app.createElement('window');\
                     globalThis.layoutDiv = app.createElement('div');\
                     layoutDiv.style.setProperty('width', '20px');\
                     layoutDiv.style.setProperty('height', '10px');\
                     layoutWindow.appendChild(layoutDiv);\
                     app.appendChild(layoutWindow);\
                     layoutDiv.getBoundingClientRect() === null",
                )
                .unwrap());
        });

        let mut layout = LayoutEngine::new(TextEngine::without_system_fonts());
        {
            let state = state.borrow();
            layout
                .compute(&state.dom, LogicalViewport::new(320.0, 240.0).unwrap())
                .unwrap();
            let computed = layout.current_shared().unwrap();
            state.publish_presented_layout(Rc::clone(&computed));
            assert!(Rc::ptr_eq(
                state.presented_layout.borrow().as_ref().unwrap(),
                &computed
            ));
        }

        context.with(|context| {
            let values: Vec<f32> = context
                .eval(
                    "const rect = layoutDiv.getBoundingClientRect();\
                     [rect.x, rect.y, rect.width, rect.height,\
                      rect.top, rect.right, rect.bottom, rect.left]",
                )
                .unwrap();
            assert_eq!(values, [0.0, 0.0, 20.0, 10.0, 0.0, 20.0, 10.0, 0.0]);
            assert!(context
                .eval::<bool, _>(
                    "const firstRect = layoutDiv.getBoundingClientRect();\
                     const secondRect = layoutDiv.getBoundingClientRect();\
                     Object.isFrozen(firstRect)\
                       && firstRect !== secondRect\
                       && Reflect.set(firstRect, 'width', 99) === false\
                       && firstRect.width === 20",
                )
                .unwrap());
            assert!(context
                .eval::<bool, _>(
                    "layoutDiv.style.setProperty('width', '30px');\
                     layoutDiv.getBoundingClientRect().width === 20",
                )
                .unwrap());
        });

        {
            let state = state.borrow();
            layout
                .compute(&state.dom, LogicalViewport::new(320.0, 240.0).unwrap())
                .unwrap();
            state.publish_presented_layout(layout.current_shared().unwrap());
        }
        context.with(|context| {
            assert_eq!(
                context
                    .eval::<f32, _>("layoutDiv.getBoundingClientRect().width")
                    .unwrap(),
                30.0
            );
        });
    }

    #[test]
    fn facade_enforces_text_only_raw_children_without_partial_mutation() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "globalThis.strictParents = ['window', 'div', 'flex', 'grid']\
                       .map(tag => app.createElement(tag));\
                     globalThis.strictTexts = strictParents\
                       .map(() => app.createTextNode('invalid'));\
                     globalThis.strictGuard = app.createElement('div');\
                     globalThis.strictGuardChild = app.createElement('grid');\
                     strictGuard.appendChild(strictGuardChild);",
                )
                .unwrap();
        });
        let revision = plugin.state().dom.revision();

        context.with(|context| {
            let results: Vec<String> = context
                .eval(
                    "(() => {\
                       const results = [];\
                       for (let index = 0; index < strictParents.length; index++) {\
                         try {\
                           strictParents[index].appendChild(strictTexts[index]);\
                           results.push('no error');\
                         } catch (error) {\
                           results.push(error.name);\
                         }\
                       }\
                       try {\
                         strictGuard.textContent = 'invalid';\
                         results.push('no error');\
                       } catch (error) {\
                         results.push(error.name);\
                       }\
                       results.push(String(strictParents.every((parent, index) =>\
                         parent.childNodes.length === 0\
                           && strictTexts[index].parentNode === null)));\
                       results.push(String(strictGuard.firstChild === strictGuardChild));\
                       return results;\
                     })()",
                )
                .unwrap();
            assert_eq!(
                results,
                [
                    "HierarchyRequestError",
                    "HierarchyRequestError",
                    "HierarchyRequestError",
                    "HierarchyRequestError",
                    "InvalidNodeTypeError",
                    "true",
                    "true",
                ]
            );
        });
        assert_eq!(plugin.state().dom.revision(), revision);

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(
                    "globalThis.strictOuter = app.createElement('text');\
                     globalThis.strictInner = app.createElement('text');\
                     globalThis.strictOuterRaw = app.createTextNode('outer ');\
                     globalThis.strictInnerRaw = app.createTextNode('inner');\
                     strictOuter.appendChild(strictOuterRaw);\
                     strictInner.appendChild(strictInnerRaw);\
                     strictOuter.appendChild(strictInner);\
                     strictOuter.childNodes.length === 2\
                       && strictOuter.firstChild === strictOuterRaw\
                       && strictOuter.lastChild === strictInner\
                       && strictInner.firstChild === strictInnerRaw\
                       && strictOuter.textContent === 'outer inner'",
                )
                .unwrap());
        });
    }

    #[test]
    fn unreachable_detached_wrappers_release_and_reclaim_during_host_maintenance() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "(() => {\
                       const detached = app.createElement('div');\
                       globalThis.detachedWeakRef = new WeakRef(detached);\
                     })()",
                )
                .unwrap();
        });
        assert_eq!(plugin.state().dom.node_count(), 2);
        // NativeNode::drop releases the root when QuickJS drops the wrapper;
        // no FinalizationRegistry callback or JavaScript job is needed.
        assert_eq!(plugin.state().live_wrapper_count(), 1);

        plugin.reclaim_for_test();
        assert_eq!(plugin.state().dom.node_count(), 1);
        assert_eq!(plugin.state().last_reclaim.nodes.len(), 1);
        assert_eq!(plugin.state().dom.iter().count(), 1);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>("detachedWeakRef.deref() === undefined")
                .unwrap());
        });
    }

    #[test]
    fn a_live_descendant_wrapper_retains_its_complete_detached_component() {
        let (plugin, _) = DomPlugin::new();
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "(() => {\
                       const root = app.createElement('div');\
                       const child = app.createElement('div');\
                       const sibling = app.createElement('grid');\
                       root.appendChild(child);\
                       root.appendChild(sibling);\
                       globalThis.keptChild = child;\
                     })()",
                )
                .unwrap();
        });
        assert_eq!(plugin.state().dom.node_count(), 4);

        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrapper_count(), 2);
        plugin.reclaim_for_test();
        assert_eq!(plugin.state().dom.node_count(), 4);
        assert!(plugin.state().last_reclaim.nodes.is_empty());

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(
                    "(() => {\
                       const root = keptChild.parentNode;\
                       return root.childNodes.length === 2\
                         && root.firstChild === keptChild\
                         && root.lastChild.localName === 'grid';\
                     })()",
                )
                .unwrap());
            context
                .eval::<(), _>("delete globalThis.keptChild")
                .unwrap();
        });
        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrapper_count(), 1);

        plugin.reclaim_for_test();
        assert_eq!(plugin.state().dom.node_count(), 1);
        assert_eq!(plugin.state().last_reclaim.nodes.len(), 3);
    }

    #[test]
    fn text_content_replacement_keeps_a_wrapped_old_child_valid_and_detached() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            assert!(context
                .eval::<bool, _>(
                    "globalThis.windowNode = app.createElement('window');\
                     globalThis.parentNode = app.createElement('text');\
                     globalThis.oldText = app.createTextNode('old');\
                     parentNode.appendChild(oldText);\
                     windowNode.appendChild(parentNode);\
                     app.appendChild(windowNode);\
                     parentNode.textContent = 'new';\
                     oldText.parentNode === null\
                       && oldText.data === 'old'\
                       && parentNode.firstChild !== oldText\
                       && parentNode.firstChild.data === 'new'",
                )
                .unwrap());
        });

        plugin.reclaim_for_test();
        let state = plugin.state();
        let dom = &state.dom;
        let window = dom.children(dom.root()).unwrap()[0];
        let parent = dom.children(window).unwrap()[0];
        let replacement = dom.children(parent).unwrap()[0];
        assert_eq!(dom.text(replacement), Some("new"));
        assert_eq!(dom.text_content(parent), Ok("new".into()));
        assert_eq!(dom.text_content(replacement), Ok("new".into()));
    }

    #[test]
    fn wrapper_listener_cycles_do_not_retain_detached_native_nodes() {
        let (plugin, _) = DomPlugin::new();
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "(() => {\
                       const node = app.createElement('div');\
                       const listener = () => node.localName;\
                       node.addEventListener('click', listener);\
                       globalThis.listenerCycleWeakRef = new WeakRef(node);\
                     })()",
                )
                .unwrap();
        });
        assert_eq!(plugin.state().dom.node_count(), 2);

        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrapper_count(), 1);
        plugin.reclaim_for_test();
        assert_eq!(plugin.state().dom.node_count(), 1);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>("listenerCycleWeakRef.deref() === undefined")
                .unwrap());
        });
    }

    #[test]
    fn connected_listener_wrapper_is_rooted_only_while_attached() {
        let (plugin, _) = DomPlugin::new();
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "globalThis.connectedListenerCalls = 0;\
                     globalThis.weakRefDeref = WeakRef.prototype.deref;\
                     WeakRef.prototype.deref = () => { throw new Error('overridden deref') };\
                     (() => {\
                       const windowNode = app.createElement('window');\
                       const beforeAttach = app.createElement('div');\
                       const afterAttach = app.createElement('div');\
                       beforeAttach.addEventListener('click',\
                         () => connectedListenerCalls++);\
                       globalThis.beforeAttachWeakRef = new WeakRef(beforeAttach);\
                       globalThis.afterAttachWeakRef = new WeakRef(afterAttach);\
                       windowNode.appendChild(beforeAttach);\
                       windowNode.appendChild(afterAttach);\
                       app.appendChild(windowNode);\
                       afterAttach.addEventListener('click',\
                         () => connectedListenerCalls++);\
                     })()",
                )
                .unwrap();
        });
        let (targets, presented_revision) = {
            let state = plugin.state();
            let window = state.dom.children(state.dom.root()).unwrap()[0];
            (
                state.dom.children(window).unwrap().to_vec(),
                state.dom.revision(),
            )
        };

        collect_garbage(&runtime, &context);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>(
                    "weakRefDeref.call(beforeAttachWeakRef) !== undefined\
                       && weakRefDeref.call(afterAttachWeakRef) !== undefined",
                )
                .unwrap());
            for target in targets.iter().copied() {
                classes::dispatch_mouse_event(
                    &context,
                    NativeMouseEvent {
                        event_type: "click",
                        target,
                        presented_revision,
                        client_x: 0.0,
                        client_y: 0.0,
                        button: 0,
                        buttons: 0,
                        related_target: None,
                    },
                )
                .unwrap();
            }
            assert_eq!(context.eval::<u32, _>("connectedListenerCalls").unwrap(), 2);
            context
                .eval::<(), _>("app.removeChild(app.firstChild)")
                .unwrap();
        });

        collect_garbage(&runtime, &context);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>(
                    "weakRefDeref.call(beforeAttachWeakRef) === undefined\
                       && weakRefDeref.call(afterAttachWeakRef) === undefined",
                )
                .unwrap());
        });
        plugin.reclaim_for_test();
        assert!(targets
            .into_iter()
            .all(|target| plugin.state().dom.node(target).is_none()));
    }

    #[test]
    fn mouse_dispatch_preserves_payload_and_hover_propagation() {
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
                for (const type of ['click', 'mousedown', 'mouseup', 'mousemove',
                                    'mouseover', 'mouseout', 'mouseenter', 'mouseleave']) {
                    mouseTarget.addEventListener(type, function (event) {
                        lastMouseEvent = event;
                        mouseCalls.push('target');
                        mouseChecks.push(event.type === type,
                            event.target === mouseTarget,
                            event.currentTarget === mouseTarget, this === mouseTarget,
                            event.clientX === 12.5, event.clientY === 8.25,
                            event.button === 2, event.buttons === 3,
                            event.relatedTarget === mouseRelated);
                        try { event.buttons = 99; } catch {}
                        try { event.relatedTarget = mouseTarget; } catch {}
                        mouseChecks.push(event.buttons === 3,
                            event.relatedTarget === mouseRelated);
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
                "mousedown",
                "mouseup",
                "mousemove",
                "mouseover",
                "mouseout",
                "mouseenter",
                "mouseleave",
            ] {
                context
                    .eval::<(), _>("mouseCalls = []; mouseChecks = []")
                    .unwrap();
                classes::dispatch_mouse_event(
                    &context,
                    NativeMouseEvent {
                        event_type,
                        target,
                        presented_revision,
                        client_x: 12.5,
                        client_y: 8.25,
                        button: 2,
                        buttons: 3,
                        related_target: Some(related_target),
                    },
                )
                .unwrap();
                let bubbles = !matches!(event_type, "mouseenter" | "mouseleave");
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
                assert_eq!(flags, [bubbles, bubbles, bubbles, true], "{event_type}");
            }
            // Leaving the window has no related node.
            context
                .eval::<(), _>("mouseRelated = null; mouseChecks = []")
                .unwrap();
            let leave = NativeMouseEvent {
                event_type: "mouseleave",
                target,
                presented_revision,
                client_x: 12.5,
                client_y: 8.25,
                button: 2,
                buttons: 3,
                related_target: None,
            };
            classes::dispatch_mouse_event(&context, leave).unwrap();
            assert!(context
                .eval::<bool, _>("mouseChecks.every(Boolean)")
                .unwrap());

            // A queued event must not reach a target detached before dispatch.
            context
                .eval::<(), _>("mouseWindow.removeChild(mouseTarget); mouseCalls = []")
                .unwrap();
            classes::dispatch_mouse_event(&context, leave).unwrap();
            assert!(context.eval::<bool, _>("mouseCalls.length === 0").unwrap());
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

                state
                    .borrow()
                    .enqueue_mouse_events(vec![NativeMouseEvent {
                        event_type: "click",
                        target,
                        presented_revision,
                        client_x: 12.5,
                        client_y: 8.25,
                        button: 0,
                        buttons: 0,
                        related_target: None,
                    }])
                    .unwrap();
                state
                    .borrow()
                    .enqueue_mouse_events(vec![NativeMouseEvent {
                        event_type: "click",
                        target: immediate_target,
                        presented_revision,
                        client_x: 20.0,
                        client_y: 10.0,
                        button: 0,
                        buttons: 0,
                        related_target: None,
                    }])
                    .unwrap();
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

    #[test]
    fn a_collected_attached_wrapper_is_recreated_canonically() {
        let (plugin, _) = DomPlugin::new();
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "(() => {\
                       const first = app.createElement('window');\
                       app.appendChild(first);\
                       globalThis.oldWindowWeakRef = new WeakRef(first);\
                     })()",
                )
                .unwrap();
        });
        assert_eq!(plugin.state().live_wrapper_count(), 1);

        // The native app tree retains the node even after its JavaScript
        // wrapper is collected, so traversal can intern a fresh wrapper.
        runtime.run_gc();
        context.with(|context| {
            assert!(context
                .eval::<bool, _>(
                    "oldWindowWeakRef.deref() === undefined\
                      && (globalThis.newWindowWrapper = app.firstChild) instanceof Window",
                )
                .unwrap());
        });
        context.with(|context| while context.execute_pending_job() {});
        assert_eq!(plugin.state().live_wrapper_count(), 2);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>("app.firstChild === newWindowWrapper")
                .unwrap());
        });
    }

    #[test]
    fn repeated_detached_wrapper_cycles_return_the_arena_to_baseline() {
        let (plugin, _) = DomPlugin::new();
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "for (let index = 0; index < 100; index++) {\
                       const node = app.createElement('div');\
                       node.style.setProperty('width', `${index}px`);\
                       node.addEventListener('click', () => node.localName);\
                     }",
                )
                .unwrap();
        });
        assert_eq!(plugin.state().dom.node_count(), 101);

        collect_garbage(&runtime, &context);
        plugin.reclaim_for_test();
        assert_eq!(plugin.state().live_wrapper_count(), 1);
        assert_eq!(plugin.state().dom.node_count(), 1);
        assert_eq!(plugin.state().last_reclaim.nodes.len(), 100);
    }

    fn run_framework_fixture(prefix: &str, bundle: &str) {
        let (plugin, _) = DomPlugin::new();
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context.eval::<(), _>(bundle).unwrap();
            assert!(context
                .eval::<bool, _>(format!("{prefix}MountFixture()"))
                .catch(&context)
                .unwrap());
        });
        plugin.reclaim_for_test();
        {
            let state = plugin.state();
            let dom = &state.dom;
            let window = dom.children(dom.root()).unwrap()[0];
            let list = dom.children(window).unwrap()[0];
            assert_eq!(subtree_text(dom, list), "ABC");
        }

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(format!("{prefix}UpdateFixture()"))
                .unwrap());
        });
        plugin.reclaim_for_test();
        {
            let state = plugin.state();
            let dom = &state.dom;
            let window = dom.children(dom.root()).unwrap()[0];
            let list = dom.children(window).unwrap()[0];
            let items = dom.children(list).unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(dom.attribute(items[0], "data-id"), Some("c"));
            assert_eq!(dom.attribute(items[1], "data-id"), Some("a"));
            assert_eq!(subtree_text(dom, list), "CA updated");
        }

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(format!("{prefix}UnmountFixture()"))
                .unwrap());
        });
        plugin.reclaim_for_test();
        assert!(plugin
            .state()
            .dom
            .children(plugin.state().dom.root())
            .unwrap()
            .is_empty());

        context.with(|context| {
            context
                .eval::<(), _>(format!("{prefix}ReleaseFixtureReferences()"))
                .unwrap();
        });
        collect_garbage(&runtime, &context);
        plugin.reclaim_for_test();
        assert_eq!(plugin.state().dom.node_count(), 1);
    }

    #[test]
    fn facade_reports_named_errors_without_partial_mutation() {
        let (plugin, _) = DomPlugin::new();
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            let values: Vec<String> = context
                .eval(
                    "const windowNode = app.createElement('window');\
                     app.appendChild(windowNode);\
                     const errors = [];\
                     try { app.createElement('canvas') } catch (error) { errors.push(error.name) }\
                     try { app.appendChild(app.createElement('div')) } catch (error) { errors.push(error.name) }\
                     try { windowNode.removeChild(app.createElement('div')) } catch (error) { errors.push(error.name) }\
                     errors",
                )
                .unwrap();
            assert_eq!(
                values,
                ["TypeError", "HierarchyRequestError", "NotFoundError"]
            );
        });

        let state = plugin.state();
        assert_eq!(state.dom.children(state.dom.root()).unwrap().len(), 1);
    }
}
