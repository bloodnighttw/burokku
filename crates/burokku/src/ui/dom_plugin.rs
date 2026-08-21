//! BTS ownership and QuickJS publication for the native DOM facade.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use runtime::{
    rquickjs::{Ctx, Exception},
    Plugin, RuntimeRole,
};

use super::elements::{
    CommitNotifier, Dom, DomPublisher, NodeId, PublishedDomReader, ReclaimReport,
};

mod bindings;
mod errors;
mod lifetime;

pub(super) type SharedDomState = Arc<Mutex<DomPluginState>>;

pub(super) struct DomPluginState {
    // the staging domJavascript read and modify/
    pub(super) staging: Dom,
    // the javascript facade side of dom reference tracking
    pub(super) live_wrappers: HashMap<NodeId, usize>,
    // the node last cleanup
    pub(super) last_reclaim: ReclaimReport,
}

/// The single BTS owner of staging DOM state and immutable publication.
pub(crate) struct DomPlugin {
    state: SharedDomState,
    publisher: DomPublisher,
}

impl DomPlugin {
    /// Create a BTS DOM plugin and the corresponding MTS publication reader.
    pub(crate) fn new(notifier: impl CommitNotifier) -> (Self, PublishedDomReader) {
        let staging = Dom::new();
        let (publisher, reader) = DomPublisher::new(&staging, notifier);
        let state = Arc::new(Mutex::new(DomPluginState {
            staging,
            live_wrappers: HashMap::new(),
            last_reclaim: ReclaimReport::default(),
        }));

        (Self { state, publisher }, reader)
    }

    #[cfg(test)]
    fn state(&self) -> std::sync::MutexGuard<'_, DomPluginState> {
        self.state.lock().expect("DOM plugin state is not poisoned")
    }
}

impl Plugin for DomPlugin {
    fn name(&self) -> &'static str {
        "burokku-dom"
    }

    fn install<'js>(&self, context: &Ctx<'js>) -> runtime::Result<()> {
        if RuntimeRole::from_context(context) == Some(RuntimeRole::Main) {
            return Err(Exception::throw_type(
                context,
                "the DOM facade can only be installed in the background runtime",
            ));
        }
        bindings::install(context, self.state.clone())
    }

    fn checkpoint(&mut self) -> runtime::Result<()> {
        let mut state = self.state.lock().map_err(|_| runtime::Error::Unknown)?;
        state.reclaim_detached()?;
        self.publisher.checkpoint(&state.staging);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use runtime::rquickjs::{CatchResultExt, Context, Runtime as JsRuntime};

    use super::*;
    use crate::ui::elements::{DomSnapshot, Element, NodeKind};

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

    fn snapshot_text(snapshot: &DomSnapshot, root: NodeId) -> String {
        let mut text = String::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if let Some(value) = snapshot.text(id) {
                text.push_str(value);
            } else if let Some(children) = snapshot.children(id) {
                pending.extend(children.iter().rev().copied());
            }
        }
        text
    }

    #[test]
    fn installs_permanent_app_and_host_only_node_classes() {
        let (plugin, _) = DomPlugin::new(|_| {});
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
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
        assert_eq!(state.live_wrappers.get(&state.staging.root()), Some(&1));
    }

    #[test]
    fn facade_mutates_staging_synchronously_and_publishes_at_checkpoint() {
        let notifications = Arc::new(AtomicUsize::new(0));
        let (mut plugin, reader) = DomPlugin::new({
            let notifications = notifications.clone();
            move |_| {
                notifications.fetch_add(1, Ordering::AcqRel);
            }
        });
        let (_runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            let values: Vec<bool> = context
                .eval(
                    "globalThis.kept = {};\
                     kept.window = app.createElement('window');\
                     kept.div = app.createElement('div');\
                     kept.text = app.createTextNode('before');\
                     kept.div.setAttribute('role', 'status');\
                     kept.div.style.setProperty('width', '20px');\
                     kept.div.appendChild(kept.text);\
                     kept.window.appendChild(kept.div);\
                     app.appendChild(kept.window);\
                     kept.text.data = 'after';\
                     [\
                       app.firstChild === kept.window,\
                       kept.window.firstChild === kept.div,\
                       kept.div.firstChild === kept.text,\
                       kept.text.parentNode === kept.div,\
                       kept.text.textContent === 'after',\
                       kept.div.getAttribute('role') === 'status',\
                       kept.div.style === kept.div.style,\
                       kept.text.isConnected\
                     ]",
                )
                .unwrap();
            assert_eq!(values, [true, true, true, true, true, true, true, true]);
        });

        assert_eq!(reader.load().revision(), 0);
        plugin.checkpoint().unwrap();
        let published = reader.load();
        let snapshot = published.snapshot();
        let window = snapshot.children(snapshot.root()).unwrap()[0];
        let div = snapshot.children(window).unwrap()[0];
        let text = snapshot.children(div).unwrap()[0];

        assert!(matches!(
            snapshot.kind(window),
            Some(NodeKind::Element(Element::Window { .. }))
        ));
        assert!(matches!(
            snapshot.kind(div),
            Some(NodeKind::Element(Element::Div { .. }))
        ));
        assert_eq!(snapshot.text(text), Some("after"));
        assert_eq!(snapshot.attribute(div, "role"), Some("status"));
        assert_eq!(notifications.load(Ordering::Acquire), 1);

        plugin.checkpoint().unwrap();
        assert_eq!(notifications.load(Ordering::Acquire), 1);
    }

    #[test]
    fn unreachable_detached_wrappers_release_and_reclaim_at_checkpoint() {
        let (mut plugin, reader) = DomPlugin::new(|_| {});
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
        assert_eq!(plugin.state().staging.node_count(), 2);
        assert_eq!(plugin.state().live_wrappers.len(), 2);

        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrappers.len(), 1);

        plugin.checkpoint().unwrap();
        assert_eq!(plugin.state().staging.node_count(), 1);
        assert_eq!(plugin.state().last_reclaim.nodes.len(), 1);
        assert_eq!(reader.load().snapshot().iter().count(), 1);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>("detachedWeakRef.deref() === undefined")
                .unwrap());
        });
    }

    #[test]
    fn a_live_descendant_wrapper_retains_its_complete_detached_component() {
        let (mut plugin, _) = DomPlugin::new(|_| {});
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context
                .eval::<(), _>(
                    "(() => {\
                       const root = app.createElement('div');\
                       const child = app.createElement('div');\
                       const sibling = app.createTextNode('sibling');\
                       root.appendChild(child);\
                       root.appendChild(sibling);\
                       globalThis.keptChild = child;\
                     })()",
                )
                .unwrap();
        });
        assert_eq!(plugin.state().staging.node_count(), 4);

        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrappers.len(), 2);
        plugin.checkpoint().unwrap();
        assert_eq!(plugin.state().staging.node_count(), 4);
        assert!(plugin.state().last_reclaim.nodes.is_empty());

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(
                    "(() => {\
                       const root = keptChild.parentNode;\
                       return root.childNodes.length === 2\
                         && root.firstChild === keptChild\
                         && root.lastChild.textContent === 'sibling';\
                     })()",
                )
                .unwrap());
            context
                .eval::<(), _>("delete globalThis.keptChild")
                .unwrap();
        });
        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrappers.len(), 1);

        plugin.checkpoint().unwrap();
        assert_eq!(plugin.state().staging.node_count(), 1);
        assert_eq!(plugin.state().last_reclaim.nodes.len(), 3);
    }

    #[test]
    fn text_content_replacement_keeps_a_wrapped_old_child_valid_and_detached() {
        let (mut plugin, reader) = DomPlugin::new(|_| {});
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

        plugin.checkpoint().unwrap();
        let snapshot = reader.load();
        let window = snapshot
            .snapshot()
            .children(snapshot.snapshot().root())
            .unwrap()[0];
        let parent = snapshot.snapshot().children(window).unwrap()[0];
        let replacement = snapshot.snapshot().children(parent).unwrap()[0];
        assert_eq!(snapshot.snapshot().text(replacement), Some("new"));
        assert_eq!(
            plugin.state().staging.text_content(parent),
            Ok("new".into())
        );
        assert_eq!(
            plugin.state().staging.text_content(replacement),
            Ok("new".into())
        );
    }

    #[test]
    fn wrapper_listener_cycles_do_not_retain_detached_native_nodes() {
        let (mut plugin, _) = DomPlugin::new(|_| {});
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
        assert_eq!(plugin.state().staging.node_count(), 2);

        collect_garbage(&runtime, &context);
        assert_eq!(plugin.state().live_wrappers.len(), 1);
        plugin.checkpoint().unwrap();
        assert_eq!(plugin.state().staging.node_count(), 1);
        context.with(|context| {
            assert!(context
                .eval::<bool, _>("listenerCycleWeakRef.deref() === undefined")
                .unwrap());
        });
    }

    #[test]
    fn a_collected_attached_wrapper_is_recreated_canonically() {
        let (plugin, _) = DomPlugin::new(|_| {});
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
        let (mut plugin, _) = DomPlugin::new(|_| {});
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
        assert_eq!(plugin.state().staging.node_count(), 101);

        collect_garbage(&runtime, &context);
        plugin.checkpoint().unwrap();
        assert_eq!(plugin.state().live_wrappers.len(), 1);
        assert_eq!(plugin.state().staging.node_count(), 1);
        assert_eq!(plugin.state().last_reclaim.nodes.len(), 100);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_checkpoint_publishes_ready_microtasks_and_successes_before_errors() {
        let notifications = Arc::new(AtomicUsize::new(0));
        let (plugin, reader) = DomPlugin::new({
            let notifications = notifications.clone();
            move |_| {
                notifications.fetch_add(1, Ordering::AcqRel);
            }
        });
        let runtime = runtime::Runtime::builder()
            .plugin(plugin)
            .build()
            .await
            .unwrap();

        runtime
            .eval::<()>(
                "globalThis.windowNode = app.createElement('window');\
                 globalThis.textNode = app.createTextNode('before');\
                 windowNode.appendChild(textNode);\
                 app.appendChild(windowNode);\
                 Promise.resolve().then(() => { textNode.data = 'microtask' })",
            )
            .await
            .unwrap();
        let first = reader.load();
        let window = first.snapshot().children(first.snapshot().root()).unwrap()[0];
        let text = first.snapshot().children(window).unwrap()[0];
        assert_eq!(first.snapshot().text(text), Some("microtask"));
        let first_revision = first.revision();

        assert!(runtime
            .eval::<()>("textNode.data = 'before-error'; throw new Error('expected')")
            .await
            .is_err());
        let second = reader.load();
        assert!(second.revision() > first_revision);
        assert_eq!(second.snapshot().text(text), Some("before-error"));
        assert_eq!(notifications.load(Ordering::Acquire), 2);

        runtime.shutdown().await.unwrap();
    }

    fn run_framework_fixture(prefix: &str, bundle: &str) {
        let notifications = Arc::new(AtomicUsize::new(0));
        let (mut plugin, reader) = DomPlugin::new({
            let notifications = notifications.clone();
            move |_| {
                notifications.fetch_add(1, Ordering::AcqRel);
            }
        });
        let (runtime, context) = context();

        context.with(|context| {
            plugin.install(&context).unwrap();
            context.eval::<(), _>(bundle).unwrap();
            assert!(context
                .eval::<bool, _>(format!("{prefix}MountFixture()"))
                .catch(&context)
                .unwrap());
        });
        plugin.checkpoint().unwrap();
        let mounted = reader.load();
        let window = mounted
            .snapshot()
            .children(mounted.snapshot().root())
            .unwrap()[0];
        let list = mounted.snapshot().children(window).unwrap()[0];
        assert_eq!(snapshot_text(mounted.snapshot(), list), "ABC");

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(format!("{prefix}UpdateFixture()"))
                .unwrap());
        });
        plugin.checkpoint().unwrap();
        let updated = reader.load();
        let window = updated
            .snapshot()
            .children(updated.snapshot().root())
            .unwrap()[0];
        let list = updated.snapshot().children(window).unwrap()[0];
        let items = updated.snapshot().children(list).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(updated.snapshot().attribute(items[0], "data-id"), Some("c"));
        assert_eq!(updated.snapshot().attribute(items[1], "data-id"), Some("a"));
        assert_eq!(snapshot_text(updated.snapshot(), list), "CA updated");

        context.with(|context| {
            assert!(context
                .eval::<bool, _>(format!("{prefix}UnmountFixture()"))
                .unwrap());
        });
        plugin.checkpoint().unwrap();
        let unmounted = reader.load();
        assert!(unmounted
            .snapshot()
            .children(unmounted.snapshot().root())
            .unwrap()
            .is_empty());

        context.with(|context| {
            context
                .eval::<(), _>(format!("{prefix}ReleaseFixtureReferences()"))
                .unwrap();
        });
        collect_garbage(&runtime, &context);
        plugin.checkpoint().unwrap();
        assert_eq!(plugin.state().staging.node_count(), 1);
        assert_eq!(notifications.load(Ordering::Acquire), 4);
    }

    #[test]
    fn facade_reports_named_errors_without_partial_mutation() {
        let (plugin, _) = DomPlugin::new(|_| {});
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
        assert_eq!(
            state.staging.children(state.staging.root()).unwrap().len(),
            1
        );
    }
}
