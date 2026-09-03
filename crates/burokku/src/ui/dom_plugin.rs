//! UI-thread ownership and QuickJS bindings for the live DOM.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use runtime::{rquickjs::Ctx, Plugin};

use super::{
    elements::{Dom, DomError, NodeId, ReclaimReport},
    layout::ComputedLayout,
};

mod classes;
mod errors;
mod lifetime;

pub(crate) type SharedUiDom = Rc<RefCell<UiDomState>>;

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
    pub(crate) live_wrappers: HashMap<NodeId, usize>,
    pub(crate) last_reclaim: ReclaimReport,
    layout: RefCell<Option<Rc<ComputedLayout>>>,
}

impl UiDomState {
    pub(crate) fn publish_layout(&self, computed: Rc<ComputedLayout>) {
        self.layout.replace(Some(computed));
    }

    pub(crate) fn layout_rect(&self, id: NodeId) -> Result<Option<LayoutRect>, DomError> {
        self.dom.element_tag(id)?;
        let layout = self.layout.borrow();
        Ok(layout
            .as_deref()
            .filter(|layout| layout.revision() == self.dom.revision())
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
            live_wrappers: HashMap::new(),
            last_reclaim: ReclaimReport::default(),
            layout: RefCell::new(None),
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
        assert_eq!(state.live_wrappers.len(), 1);
        assert_eq!(state.live_wrappers.get(&state.dom.root()), Some(&1));
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
    fn element_exposes_only_current_read_only_layout_rects() {
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
            state.publish_layout(Rc::clone(&computed));
            assert!(Rc::ptr_eq(
                state.layout.borrow().as_ref().unwrap(),
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
                     layoutDiv.getBoundingClientRect() === null",
                )
                .unwrap());
        });

        {
            let state = state.borrow();
            layout
                .compute(&state.dom, LogicalViewport::new(320.0, 240.0).unwrap())
                .unwrap();
            state.publish_layout(layout.current_shared().unwrap());
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
        let (runtime, context) = context();

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
        assert_eq!(plugin.state().live_wrappers.len(), 2);

        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrappers.len(), 1);

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
        assert_eq!(plugin.state().live_wrappers.len(), 2);
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
        assert_eq!(plugin.state().live_wrappers.len(), 1);

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
        assert_eq!(plugin.state().live_wrappers.len(), 1);
        plugin.reclaim_for_test();
        assert_eq!(plugin.state().dom.node_count(), 1);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>("listenerCycleWeakRef.deref() === undefined")
                .unwrap());
        });
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
        assert_eq!(plugin.state().live_wrappers.len(), 2);

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
        assert_eq!(plugin.state().live_wrappers.len(), 2);
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
        assert_eq!(plugin.state().live_wrappers.len(), 1);
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
