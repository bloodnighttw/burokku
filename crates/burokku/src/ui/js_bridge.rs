use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use runtime::{
    rquickjs::{prelude::Func, Ctx, Exception, Object},
    JsOptions, Plugin, Result as RuntimeResult, RuntimeRole,
};
use slotmap::{Key, KeyData};

use super::elements::{BtsDom, Dom, DomError, Elements, NodeId, SharedDom};

const NATIVE_DOM: &str = "__burokkuDomNative";
const DOM_SHIM: &str = include_str!("scripts/dom.js");

#[derive(Debug)]
struct DomState {
    owner: BtsDom,
    body: NodeId,
    // Counts live JavaScript wrappers, not references to the wrapper in JS.
    wrapper_leases: HashMap<NodeId, usize>,
    needs_reclaim: bool,
}

impl DomState {
    fn release_wrapper(&mut self, id: NodeId) {
        let Some(count) = self.wrapper_leases.get_mut(&id) else {
            return;
        };
        if *count > 1 {
            *count -= 1;
        } else {
            self.wrapper_leases.remove(&id);
        }
        self.needs_reclaim = true;
    }

    fn reclaim_unreachable(&mut self) {
        let reclaimable_roots = {
            let dom = self.owner.staging();
            let retained_roots: HashSet<_> = self
                .wrapper_leases
                .keys()
                .copied()
                .filter(|id| dom.contains(*id))
                .map(|mut id| {
                    while let Some(parent) = dom.parent(id) {
                        id = parent;
                    }
                    id
                })
                .collect();
            dom.detached_roots()
                .filter(|root| !retained_roots.contains(root))
                .collect::<Vec<_>>()
        };

        for root in reclaimable_roots {
            self.owner
                .mutate()
                .remove_subtree(root)
                .expect("a detached garbage root remains removable");
        }
    }
}

/// Owns BTS staging state, exposes DOM host operations, and publishes the
/// staging tree at runtime checkpoints.
#[derive(Clone, Debug)]
pub struct DomPlugin {
    state: Arc<Mutex<DomState>>,
}

impl DomPlugin {
    pub fn new(shared: SharedDom) -> Self {
        let mut owner = BtsDom::new(shared);
        let body = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let body = dom.create(Elements::Window {
                style: Box::default(),
            });
            dom.append_child(root, body)
                .expect("a new DOM accepts its initial Window");
            body
        };
        Self {
            state: Arc::new(Mutex::new(DomState {
                owner,
                body,
                wrapper_leases: HashMap::new(),
                needs_reclaim: false,
            })),
        }
    }

    pub fn with_new_dom() -> (Self, SharedDom) {
        let shared = SharedDom::new();
        (Self::new(shared.clone()), shared)
    }
}

impl Plugin for DomPlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> RuntimeResult<()> {
        if RuntimeRole::from_context(context) != Some(RuntimeRole::Background) {
            return Err(Exception::throw_message(
                context,
                "the DOM plugin may only be installed in the background runtime",
            ));
        }

        let native = Object::new(context.clone())?;
        install_queries(&native, &self.state)?;
        install_mutations(&native, &self.state)?;
        context.globals().set(NATIVE_DOM, native)?;
        context.eval::<(), _>(DOM_SHIM)
    }

    fn checkpoint<'js>(&mut self, context: &Ctx<'js>) -> RuntimeResult<()> {
        let mut state = state_lock(context, &self.state)?;
        // Finalizer jobs have released their wrapper leases by this point.
        // Only scan after a lease release or an operation that can leave a
        // detached component; ordinary checkpoints stay O(1) here.
        if state.needs_reclaim {
            state.reclaim_unreachable();
            state.needs_reclaim = false;
        }
        state
            .owner
            .checkpoint()
            .map_err(|error| Exception::throw_message(context, &error.to_string()))?;
        Ok(())
    }
}

fn install_queries<'js>(native: &Object<'js>, state: &Arc<Mutex<DomState>>) -> RuntimeResult<()> {
    let root_state = state.clone();
    native.set(
        "root",
        Func::from(move |context: Ctx<'js>| -> RuntimeResult<String> {
            let state = state_lock(&context, &root_state)?;
            Ok(encode(state.owner.staging().root()))
        }),
    )?;

    let body_state = state.clone();
    native.set(
        "body",
        Func::from(move |context: Ctx<'js>| -> RuntimeResult<String> {
            Ok(encode(state_lock(&context, &body_state)?.body))
        }),
    )?;

    let contains_state = state.clone();
    native.set(
        "contains",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<bool> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &contains_state)?;
                Ok(state.owner.staging().contains(id))
            },
        ),
    )?;

    let node_type_state = state.clone();
    native.set(
        "nodeType",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<u8> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &node_type_state)?;
                let element = require_element(&context, state.owner.staging(), id)?;
                Ok(if matches!(element, Elements::_String { .. }) {
                    3
                } else {
                    1
                })
            },
        ),
    )?;

    let node_name_state = state.clone();
    native.set(
        "nodeName",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<String> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &node_name_state)?;
                let element = require_element(&context, state.owner.staging(), id)?;
                Ok(element_name(element).into())
            },
        ),
    )?;

    let parent_state = state.clone();
    native.set(
        "parent",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<JsOptions<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &parent_state)?;
                require_element(&context, state.owner.staging(), id)?;
                Ok(match state.owner.staging().parent(id) {
                    Some(parent) => JsOptions::Some(encode(parent)),
                    None => JsOptions::Null,
                })
            },
        ),
    )?;

    let first_child_state = state.clone();
    native.set(
        "firstChild",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<JsOptions<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &first_child_state)?;
                let children = require_children(&context, state.owner.staging(), id)?;
                Ok(match children.first().copied() {
                    Some(child) => JsOptions::Some(encode(child)),
                    None => JsOptions::Null,
                })
            },
        ),
    )?;

    let next_sibling_state = state.clone();
    native.set(
        "nextSibling",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<JsOptions<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &next_sibling_state)?;
                let dom = state.owner.staging();
                require_element(&context, dom, id)?;
                let Some(parent) = dom.parent(id) else {
                    return Ok(JsOptions::Null);
                };
                let children = require_children(&context, dom, parent)?;
                let index = children
                    .iter()
                    .position(|child| *child == id)
                    .ok_or_else(|| {
                        dom_exception(&context, "node is absent from its parent's children")
                    })?;
                Ok(match children.get(index + 1).copied() {
                    Some(sibling) => JsOptions::Some(encode(sibling)),
                    None => JsOptions::Null,
                })
            },
        ),
    )?;

    let children_state = state.clone();
    native.set(
        "children",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<Vec<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &children_state)?;
                Ok(require_children(&context, state.owner.staging(), id)?
                    .iter()
                    .copied()
                    .map(encode)
                    .collect())
            },
        ),
    )?;

    let text_state = state.clone();
    native.set(
        "textContent",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<String> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &text_state)?;
                let mut text = String::new();
                collect_text(&context, state.owner.staging(), id, &mut text)?;
                Ok(text)
            },
        ),
    )?;

    let attribute_state = state.clone();
    native.set(
        "getAttribute",
        Func::from(
            move |context: Ctx<'js>,
                  handle: String,
                  name: String|
                  -> RuntimeResult<JsOptions<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &attribute_state)?;
                require_element(&context, state.owner.staging(), id)?;
                Ok(match state.owner.staging().attribute(id, &name) {
                    Some(value) => JsOptions::Some(value.to_owned()),
                    None => JsOptions::Null,
                })
            },
        ),
    )?;

    let style_support_state = state.clone();
    native.set(
        "supportsStyle",
        Func::from(
            move |context: Ctx<'js>, handle: String, name: String| -> RuntimeResult<bool> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &style_support_state)?;
                map_dom_result(
                    &context,
                    state.owner.staging().supports_style_property(id, &name),
                )
            },
        ),
    )?;

    let style_declarations_state = state.clone();
    native.set(
        "styleDeclarations",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<Vec<Vec<String>>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &style_declarations_state)?;
                require_element(&context, state.owner.staging(), id)?;
                Ok(state
                    .owner
                    .staging()
                    .style_declarations(id)
                    .expect("a required element has a DOM node")
                    .iter()
                    .map(|(name, value)| vec![name.clone(), value.clone()])
                    .collect())
            },
        ),
    )?;

    Ok(())
}

fn install_mutations<'js>(native: &Object<'js>, state: &Arc<Mutex<DomState>>) -> RuntimeResult<()> {
    let retain_state = state.clone();
    native.set(
        "retain",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &retain_state)?;
                require_element(&context, state.owner.staging(), id)?;
                let count = state.wrapper_leases.entry(id).or_default();
                *count = count.checked_add(1).ok_or_else(|| {
                    dom_exception(&context, "DOM node wrapper lease count overflowed")
                })?;
                Ok(())
            },
        ),
    )?;

    let release_state = state.clone();
    native.set(
        "release",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                state_lock(&context, &release_state)?.release_wrapper(id);
                Ok(())
            },
        ),
    )?;

    let create_element_state = state.clone();
    native.set(
        "createElement",
        Func::from(
            move |context: Ctx<'js>, tag: String| -> RuntimeResult<String> {
                let element = element_for_tag(&context, &tag)?;
                let mut state = state_lock(&context, &create_element_state)?;
                let id = state.owner.mutate().create(element);
                // If wrapper construction fails after this host call, the new
                // detached native node has no lease and must still be swept.
                state.needs_reclaim = true;
                Ok(encode(id))
            },
        ),
    )?;

    let create_text_state = state.clone();
    native.set(
        "createTextNode",
        Func::from(
            move |context: Ctx<'js>, text: String| -> RuntimeResult<String> {
                let mut state = state_lock(&context, &create_text_state)?;
                let id = state
                    .owner
                    .mutate()
                    .create(Elements::_String { string: text });
                state.needs_reclaim = true;
                Ok(encode(id))
            },
        ),
    )?;

    let append_state = state.clone();
    native.set(
        "append",
        Func::from(
            move |context: Ctx<'js>, parent: String, child: String| -> RuntimeResult<()> {
                let parent = decode(&context, &parent)?;
                let child = decode(&context, &child)?;
                let mut state = state_lock(&context, &append_state)?;
                let result = state.owner.mutate().append_child(parent, child);
                map_dom_result(&context, result)
            },
        ),
    )?;

    let insert_state = state.clone();
    native.set(
        "insertBefore",
        Func::from(
            move |context: Ctx<'js>,
                  parent: String,
                  child: String,
                  before: Option<String>|
                  -> RuntimeResult<()> {
                let parent = decode(&context, &parent)?;
                let child = decode(&context, &child)?;
                let before = before
                    .as_deref()
                    .map(|handle| decode(&context, handle))
                    .transpose()?;
                let mut state = state_lock(&context, &insert_state)?;
                insert_before(&context, &mut state.owner, parent, child, before)
            },
        ),
    )?;

    let remove_child_state = state.clone();
    native.set(
        "removeChild",
        Func::from(
            move |context: Ctx<'js>, parent: String, child: String| -> RuntimeResult<()> {
                let parent = decode(&context, &parent)?;
                let child = decode(&context, &child)?;
                let mut state = state_lock(&context, &remove_child_state)?;
                if state.owner.staging().parent(child) != Some(parent) {
                    return Err(dom_exception(
                        &context,
                        "the node to remove is not a child of this parent",
                    ));
                }
                let result = state.owner.mutate().detach(child);
                if result.is_ok() {
                    state.needs_reclaim = true;
                }
                map_dom_result(&context, result)
            },
        ),
    )?;

    let replace_state = state.clone();
    native.set(
        "replaceChildren",
        Func::from(
            move |context: Ctx<'js>, parent: String, children: Vec<String>| -> RuntimeResult<()> {
                let parent = decode(&context, &parent)?;
                let children = children
                    .iter()
                    .map(|handle| decode(&context, handle))
                    .collect::<RuntimeResult<Vec<_>>>()?;
                let mut state = state_lock(&context, &replace_state)?;
                replace_children(&context, &mut state.owner, parent, &children)?;
                state.needs_reclaim = true;
                Ok(())
            },
        ),
    )?;

    let text_state = state.clone();
    native.set(
        "setTextContent",
        Func::from(
            move |context: Ctx<'js>, handle: String, text: String| -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &text_state)?;
                let can_detach_children = !matches!(
                    state.owner.staging().element(id),
                    Some(Elements::_String { .. })
                );
                set_text_content(&context, &mut state.owner, id, text)?;
                if can_detach_children {
                    state.needs_reclaim = true;
                }
                Ok(())
            },
        ),
    )?;

    let set_attribute_state = state.clone();
    native.set(
        "setAttribute",
        Func::from(
            move |context: Ctx<'js>,
                  handle: String,
                  name: String,
                  value: String|
                  -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &set_attribute_state)?;
                let result = state.owner.mutate().set_attribute(id, name, value);
                map_dom_result(&context, result)
            },
        ),
    )?;

    let remove_attribute_state = state.clone();
    native.set(
        "removeAttribute",
        Func::from(
            move |context: Ctx<'js>, handle: String, name: String| -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &remove_attribute_state)?;
                let result = state.owner.mutate().remove_attribute(id, &name);
                map_dom_result(&context, result).map(drop)
            },
        ),
    )?;

    let set_style_state = state.clone();
    native.set(
        "setStyle",
        Func::from(
            move |context: Ctx<'js>,
                  handle: String,
                  name: String,
                  value: String|
                  -> RuntimeResult<bool> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &set_style_state)?;
                let result = state.owner.mutate().set_style_property(id, &name, &value);
                map_dom_result(&context, result)
            },
        ),
    )?;

    let remove_style_state = state.clone();
    native.set(
        "removeStyle",
        Func::from(
            move |context: Ctx<'js>, handle: String, name: String| -> RuntimeResult<bool> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &remove_style_state)?;
                let result = state.owner.mutate().remove_style_property(id, &name);
                map_dom_result(&context, result)
            },
        ),
    )?;

    Ok(())
}

fn state_lock<'a, 'js>(
    context: &Ctx<'js>,
    state: &'a Arc<Mutex<DomState>>,
) -> RuntimeResult<std::sync::MutexGuard<'a, DomState>> {
    state
        .lock()
        .map_err(|_| dom_exception(context, "the BTS DOM state is poisoned"))
}

fn encode(id: NodeId) -> String {
    id.data().as_ffi().to_string()
}

fn decode(context: &Ctx<'_>, handle: &str) -> RuntimeResult<NodeId> {
    let raw = handle
        .parse::<u64>()
        .map_err(|_| dom_exception(context, "invalid DOM node handle"))?;
    Ok(NodeId::from(KeyData::from_ffi(raw)))
}

fn require_element<'a>(context: &Ctx<'_>, dom: &'a Dom, id: NodeId) -> RuntimeResult<&'a Elements> {
    dom.element(id)
        .ok_or_else(|| dom_exception(context, &format!("node {id:?} is stale")))
}

fn require_children<'a>(
    context: &Ctx<'_>,
    dom: &'a Dom,
    id: NodeId,
) -> RuntimeResult<&'a [NodeId]> {
    dom.children(id)
        .ok_or_else(|| dom_exception(context, &format!("node {id:?} is stale")))
}

fn element_for_tag(context: &Ctx<'_>, tag: &str) -> RuntimeResult<Elements> {
    match tag {
        "window" => Ok(Elements::Window {
            style: Box::default(),
        }),
        "div" => Ok(Elements::Div {
            style: Box::default(),
        }),
        "flex" => Ok(Elements::Flex {
            style: Box::default(),
        }),
        "grid" => Ok(Elements::Grid {
            style: Box::default(),
        }),
        "text" => Ok(Elements::Text {
            style: Box::default(),
        }),
        _ => Err(dom_exception(
            context,
            &format!("unsupported element <{tag}>"),
        )),
    }
}

fn element_name(element: &Elements) -> &'static str {
    match element {
        Elements::App => "APP",
        Elements::Window { .. } => "WINDOW",
        Elements::Div { .. } => "DIV",
        Elements::Flex { .. } => "FLEX",
        Elements::Grid { .. } => "GRID",
        Elements::Text { .. } => "TEXT",
        Elements::_String { .. } => "#text",
    }
}

fn insert_before(
    context: &Ctx<'_>,
    owner: &mut BtsDom,
    parent: NodeId,
    child: NodeId,
    before: Option<NodeId>,
) -> RuntimeResult<()> {
    let index = {
        let dom = owner.staging();
        require_element(context, dom, child)?;
        let children = require_children(context, dom, parent)?;
        if before == Some(child) {
            if dom.parent(child) != Some(parent) {
                return Err(dom_exception(
                    context,
                    "the reference node is not a child of this parent",
                ));
            }
            return Ok(());
        }
        match before {
            Some(before) => {
                if dom.parent(before) != Some(parent) {
                    return Err(dom_exception(
                        context,
                        "the reference node is not a child of this parent",
                    ));
                }
                children
                    .iter()
                    .filter(|existing| **existing != child)
                    .position(|existing| *existing == before)
                    .ok_or_else(|| dom_exception(context, "reference node was not found"))?
            }
            None => children.len() - usize::from(dom.parent(child) == Some(parent)),
        }
    };
    let result = owner.mutate().insert_child(parent, index, child);
    map_dom_result(context, result)
}

fn replace_children(
    context: &Ctx<'_>,
    owner: &mut BtsDom,
    parent: NodeId,
    children: &[NodeId],
) -> RuntimeResult<()> {
    // Run the exact replacement against a cheap copy-on-write clone first. A
    // later child can invalidate the whole operation (for example, when it is
    // `parent` itself), so validating each append while mutating staging would
    // expose a partially detached/replaced tree at the next checkpoint.
    let mut validation = owner.staging().clone();
    map_dom_result(
        context,
        apply_replace_children(&mut validation, parent, children),
    )?;

    apply_replace_children(&mut owner.mutate(), parent, children)
        .expect("replaceChildren was validated against identical staging state");
    Ok(())
}

fn apply_replace_children(
    dom: &mut Dom,
    parent: NodeId,
    children: &[NodeId],
) -> Result<(), DomError> {
    let existing = dom
        .children(parent)
        .ok_or(DomError::NodeNotFound(parent))?
        .to_vec();
    for child in existing {
        dom.detach(child)?;
    }
    for child in children {
        dom.append_child(parent, *child)?;
    }
    Ok(())
}

fn collect_text(
    context: &Ctx<'_>,
    dom: &Dom,
    id: NodeId,
    output: &mut String,
) -> RuntimeResult<()> {
    match require_element(context, dom, id)? {
        Elements::_String { string } => output.push_str(string),
        _ => {
            for child in require_children(context, dom, id)? {
                collect_text(context, dom, *child, output)?;
            }
        }
    }
    Ok(())
}

fn set_text_content(
    context: &Ctx<'_>,
    owner: &mut BtsDom,
    id: NodeId,
    text: String,
) -> RuntimeResult<()> {
    let element = require_element(context, owner.staging(), id)?.clone();
    if matches!(element, Elements::_String { .. }) {
        return map_dom_result(
            context,
            owner
                .mutate()
                .set_element(id, Elements::_String { string: text }),
        );
    }
    if matches!(element, Elements::App) {
        return Err(dom_exception(context, "App textContent cannot be changed"));
    }

    let children = require_children(context, owner.staging(), id)?.to_vec();
    for child in children {
        map_dom_result(context, owner.mutate().detach(child))?;
    }
    if text.is_empty() {
        return Ok(());
    }

    let string = owner.mutate().create(Elements::_String { string: text });
    if matches!(element, Elements::Text { .. }) {
        return map_dom_result(context, owner.mutate().append_child(id, string));
    }

    let text_element = owner.mutate().create(Elements::Text {
        style: Box::default(),
    });
    map_dom_result(context, owner.mutate().append_child(text_element, string))?;
    map_dom_result(context, owner.mutate().append_child(id, text_element))
}

fn map_dom_result<T>(context: &Ctx<'_>, result: Result<T, DomError>) -> RuntimeResult<T> {
    result.map_err(|error| dom_exception(context, &error.to_string()))
}

fn dom_exception(context: &Ctx<'_>, message: &str) -> runtime::rquickjs::Error {
    Exception::throw_message(context, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::Runtime;

    async fn runtime_with_dom() -> (Runtime, SharedDom) {
        let (dom_plugin, shared) = DomPlugin::with_new_dom();
        let runtime = Runtime::builder()
            .role(RuntimeRole::Background)
            .plugin(dom_plugin)
            .build()
            .await
            .unwrap();
        (runtime, shared)
    }

    async fn runtime_with_tracked_dom() -> (Runtime, DomPlugin, SharedDom) {
        let (dom_plugin, shared) = DomPlugin::with_new_dom();
        let runtime = Runtime::builder()
            .role(RuntimeRole::Background)
            .plugin(dom_plugin.clone())
            .build()
            .await
            .unwrap();
        (runtime, dom_plugin, shared)
    }

    async fn collect_garbage(runtime: &Runtime) {
        let (completed, completion) = tokio::sync::oneshot::channel();
        runtime
            .macrotask_queue()
            .enqueue(move |context| {
                context.run_gc();
                let _ = completed.send(());
                Ok(())
            })
            .await
            .unwrap();
        completion.await.unwrap();

        // The event loop drains finalizer jobs and runs plugin checkpoints
        // after the GC macrotask. A following macrotask starts only once both
        // phases have completed.
        runtime.eval::<()>("void 0").await.unwrap();
    }

    fn body(dom: &Dom) -> NodeId {
        dom.children(dom.root()).unwrap()[0]
    }

    #[tokio::test(flavor = "current_thread")]
    async fn javascript_can_build_and_read_a_dom_tree() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        let values: Vec<String> = runtime
            .eval(
                r#"
                const div = document.createElement("div");
                const text = document.createElement("text");
                const content = document.createTextNode("hello");
                text.appendChild(content);
                div.appendChild(text);
                document.body.appendChild(div);
                [div.nodeName, String(content.nodeType), String(div.firstChild === text),
                 String(content.parentNode === text), document.body.textContent]
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert_eq!(values, ["DIV", "3", "true", "true", "hello"]);
        let snapshot = shared.load();
        assert_eq!(snapshot.revision(), 1);
        assert_eq!(
            snapshot
                .dom()
                .children(snapshot.dom().root())
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn redispatching_an_event_resets_node_dispatch_state() {
        let (runtime, _shared) = runtime_with_dom().await;

        let calls: Vec<String> = runtime
            .eval(
                r#"
                const first = document.createElement("div");
                const parent = document.createElement("div");
                const second = document.createElement("div");
                parent.appendChild(second);
                const event = new Event("ping", { bubbles: true });
                const calls = [];

                first.addEventListener("ping", dispatched => {
                    calls.push(`first:${dispatched.target === first}`);
                    dispatched.stopImmediatePropagation();
                });
                first.addEventListener("ping", () => calls.push("first-skipped"));
                second.addEventListener("ping", dispatched => {
                    calls.push(`second:${dispatched.target === second}`);
                });
                second.addEventListener("ping", () => calls.push("second-again"));
                parent.addEventListener("ping", () => calls.push("parent"));

                first.dispatchEvent(event);
                second.dispatchEvent(event);
                calls;
                "#,
            )
            .await
            .unwrap();

        assert_eq!(
            calls,
            ["first:true", "second:true", "second-again", "parent"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn redispatching_an_event_resets_document_dispatch_state() {
        let (runtime, _shared) = runtime_with_dom().await;

        let calls: Vec<String> = runtime
            .eval(
                r#"
                const node = document.createElement("div");
                const event = new Event("ping");
                const calls = [];

                node.addEventListener("ping", dispatched => {
                    calls.push(`node:${dispatched.target === node}`);
                    dispatched.stopImmediatePropagation();
                });
                document.addEventListener("ping", dispatched => {
                    calls.push(`document:${dispatched.target === document}`);
                });
                document.addEventListener("ping", () => calls.push("document-again"));

                node.dispatchEvent(event);
                document.dispatchEvent(event);
                calls;
                "#,
            )
            .await
            .unwrap();

        assert_eq!(calls, ["node:true", "document:true", "document-again"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nested_microtasks_publish_one_complete_revision() {
        let (runtime, shared) = runtime_with_dom().await;
        let old = shared.load();
        let mut commits = shared.subscribe();

        runtime
            .eval::<()>(
                r#"
                const parent = document.createElement("div");
                document.body.appendChild(parent);
                Promise.resolve().then(() => {
                    parent.appendChild(document.createElement("text"));
                    Promise.resolve().then(() => {
                        parent.firstChild.appendChild(document.createTextNode("complete"));
                    });
                });
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert_eq!(*commits.borrow_and_update(), 1);
        assert!(!commits.has_changed().unwrap());
        assert!(old.dom().children(old.dom().root()).unwrap().is_empty());

        let snapshot = shared.load();
        let parent = snapshot.dom().children(body(snapshot.dom())).unwrap()[0];
        assert_eq!(snapshot.revision(), 1);
        let text = snapshot.dom().children(parent).unwrap()[0];
        let string = snapshot.dom().children(text).unwrap()[0];
        assert!(matches!(
            snapshot.dom().element(string),
            Some(Elements::_String { string }) if string == "complete"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn javascript_exception_commits_prior_dom_mutations() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();

        assert!(runtime
            .eval::<()>(
                r#"
                const div = document.createElement("div");
                div.setAttribute("data-state", "before-error");
                document.body.appendChild(div);
                throw new Error("render failed");
                "#,
            )
            .await
            .is_err());

        commits.changed().await.unwrap();
        let snapshot = shared.load();
        let div = snapshot.dom().children(body(snapshot.dom())).unwrap()[0];
        assert_eq!(
            snapshot.dom().attribute(div, "data-state"),
            Some("before-error")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn attributes_use_dom_null_and_presence_semantics() {
        let (runtime, _shared) = runtime_with_dom().await;

        let states: Vec<bool> = runtime
            .eval(
                r#"
                const button = document.createElement("div");
                const initiallyAbsent = button.getAttribute("disabled") === null &&
                  button.hasAttribute("disabled") === false;
                button.setAttribute("disabled", "");
                const present = button.getAttribute("disabled") === "" &&
                  button.hasAttribute("disabled") === true;
                button.removeAttribute("disabled");
                const removed = button.getAttribute("disabled") === null &&
                  button.hasAttribute("disabled") === false;
                [initiallyAbsent, present, removed];
                "#,
            )
            .await
            .unwrap();

        assert_eq!(states, [true, true, true]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn attributes_are_authoritative_snapshot_data() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();

        runtime
            .eval::<()>(
                r#"
                const div = document.createElement("div");
                div.setAttribute("id", "panel");
                document.body.appendChild(div);
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        let snapshot = shared.load();
        let div = snapshot.dom().children(body(snapshot.dom())).unwrap()[0];
        assert_eq!(snapshot.dom().attribute(div, "id"), Some("panel"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn supported_styles_update_strong_element_data() {
        use crate::ui::elements::styles::{
            color::RgbaColor,
            length::{Dimension, LengthPercentage},
        };

        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        runtime
            .eval::<()>(
                r##"
                const row = document.createElement("flex");
                row.style.width = "100%";
                row.style.height = "360px";
                row.style.padding = "20px";
                row.style.gap = "16px";
                row.style.alignItems = "stretch";
                row.style.backgroundColor = "#1f2937";
                const panel = document.createElement("div");
                panel.style.flexBasis = "0px";
                panel.style.flexGrow = "2";
                panel.style.backgroundColor = "#22c55e";
                row.appendChild(panel);
                document.body.appendChild(row);
                "##,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        let snapshot = shared.load();
        let row = snapshot.dom().children(body(snapshot.dom())).unwrap()[0];
        let panel = snapshot.dom().children(row).unwrap()[0];

        let Some(Elements::Flex { style }) = snapshot.dom().element(row) else {
            panic!("row should remain a strongly typed flex element");
        };
        assert_eq!(style.common.size.width, Dimension::Percent(1.0));
        assert_eq!(style.common.size.height, Dimension::Length(360.0));
        assert_eq!(style.common.padding.left, LengthPercentage::Length(20.0));
        assert_eq!(style.gap.width, LengthPercentage::Length(16.0));
        assert_eq!(
            style.common.background_color,
            Some(RgbaColor::rgb(31, 41, 55))
        );

        let Some(Elements::Div { style }) = snapshot.dom().element(panel) else {
            panic!("panel should remain a strongly typed div element");
        };
        assert_eq!(style.flex_basis, Dimension::Length(0.0));
        assert_eq!(style.flex_grow, 2.0);
        assert_eq!(style.background_color, Some(RgbaColor::rgb(34, 197, 94)));

        let mut computed = crate::ui::computed::ComputedState::new();
        computed.compute_layout(
            &snapshot,
            taffy::geometry::Size {
                width: taffy::AvailableSpace::Definite(800.0),
                height: taffy::AvailableSpace::Definite(600.0),
            },
        );
        let row_layout = computed.layout(row).unwrap();
        let panel_layout = computed.layout(panel).unwrap();
        assert!(row_layout.size.width > 0.0 && row_layout.size.height > 0.0);
        assert!(panel_layout.size.width > 0.0 && panel_layout.size.height > 0.0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unsupported_style_properties_warn_and_are_ignored() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        let ignored: bool = runtime
            .eval(
                r#"
                const warnings = [];
                globalThis.console = { warn(message) { warnings.push(String(message)); } };
                const div = document.createElement("div");
                div.style.notDefined = "value";
                const value = div.style.notDefined;
                div.style.setProperty("also-not-defined", "value");
                const removed = div.style.removeProperty("still-not-defined");
                document.body.appendChild(div);
                warnings.length === 4 &&
                  warnings.every(message => message.includes("was ignored")) &&
                  value === "" && removed === "";
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert!(ignored);
        let snapshot = shared.load();
        assert_eq!(
            snapshot.dom().children(body(snapshot.dom())).unwrap().len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_operations_throw_without_corrupting_reachable_tree() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        let rejected: bool = runtime
            .eval(
                r#"
                const rawText = document.createTextNode("invalid here");
                let rejected = false;
                try { document.body.appendChild(rawText); }
                catch (_) { rejected = true; }
                const valid = document.createElement("div");
                document.body.appendChild(valid);
                rejected && rawText.parentNode === null && document.body.firstChild === valid;
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert!(rejected);
        let snapshot = shared.load();
        let children = snapshot.dom().children(body(snapshot.dom())).unwrap();
        assert_eq!(children.len(), 1);
        assert!(matches!(
            snapshot.dom().element(children[0]),
            Some(Elements::Div { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn insert_before_self_still_validates_the_requested_parent() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        let validated: bool = runtime
            .eval(
                r#"
                const actualParent = document.createElement("div");
                const unrelatedParent = document.createElement("div");
                const child = document.createElement("div");
                actualParent.appendChild(child);
                document.body.appendChild(actualParent);
                document.body.appendChild(unrelatedParent);

                let unrelatedRejected = false;
                try { unrelatedParent.insertBefore(child, child); }
                catch (error) {
                  unrelatedRejected = String(error).includes(
                    "reference node is not a child of this parent"
                  );
                }

                const detached = document.createElement("div");
                let detachedRejected = false;
                try { unrelatedParent.insertBefore(detached, detached); }
                catch (error) {
                  detachedRejected = String(error).includes(
                    "reference node is not a child of this parent"
                  );
                }

                let validSelfInsert = true;
                try { actualParent.insertBefore(child, child); }
                catch (_) { validSelfInsert = false; }

                unrelatedRejected && detachedRejected && validSelfInsert &&
                  child.parentNode === actualParent &&
                  actualParent.firstChild === child &&
                  unrelatedParent.firstChild === null;
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert!(validated);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_replace_children_leaves_every_node_unchanged() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        let unchanged: bool = runtime
            .eval(
                r#"
                const original = document.createElement("div");
                const firstReplacement = document.createElement("flex");
                document.body.appendChild(original);

                let cycleRejected = false;
                try { document.body.replaceChildren(firstReplacement, document.body); }
                catch (_) { cycleRejected = true; }
                const cycleWasAtomic = document.body.firstChild === original &&
                  original.parentNode === document.body &&
                  firstReplacement.parentNode === null;

                const invalidText = document.createTextNode("invalid");
                let relationshipRejected = false;
                try { document.body.replaceChildren(firstReplacement, invalidText); }
                catch (_) { relationshipRejected = true; }

                cycleRejected && cycleWasAtomic && relationshipRejected &&
                  document.body.firstChild === original &&
                  original.parentNode === document.body &&
                  firstReplacement.parentNode === null &&
                  invalidText.parentNode === null;
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert!(unchanged);
        let snapshot = shared.load();
        let children = snapshot.dom().children(body(snapshot.dom())).unwrap();
        assert_eq!(children.len(), 1);
        assert!(matches!(
            snapshot.dom().element(children[0]),
            Some(Elements::Div { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn detached_javascript_nodes_remain_valid_and_can_be_reinserted() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();
        let reusable: bool = runtime
            .eval(
                r#"
                const text = document.createElement("text");
                const content = document.createTextNode("old");
                text.appendChild(content);
                document.body.appendChild(text);
                document.body.textContent = "new";
                const detached = text.parentNode === null && content.parentNode === text;
                document.body.replaceChildren(text);
                detached && content.data === "old" && document.body.textContent === "old";
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        assert!(reusable);
        let snapshot = shared.load();
        assert_eq!(
            snapshot.dom().children(body(snapshot.dom())).unwrap().len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn repeated_text_content_reclaims_replaced_native_nodes() {
        let (runtime, dom_plugin, shared) = runtime_with_tracked_dom().await;
        let mut commits = shared.subscribe();

        runtime
            .eval::<()>(
                r#"
                for (let index = 0; index < 250; index++) {
                  document.body.textContent = `value ${index}`;
                }
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        let state = dom_plugin.state.lock().unwrap();
        // App + Window + the final Text and string nodes.
        assert_eq!(state.owner.staging().node_count(), 4);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recreated_wrappers_restore_authored_style_values() {
        let (runtime, dom_plugin, _shared) = runtime_with_tracked_dom().await;

        runtime
            .eval::<()>(
                r#"
                globalThis.styledChild = (() => {
                  const parent = document.createElement("div");
                  const child = document.createElement("div");
                  parent.style.width = "41px";
                  parent.style.flexGrow = "2.00";
                  parent.appendChild(child);
                  return child;
                })();
                "#,
            )
            .await
            .unwrap();
        collect_garbage(&runtime).await;

        // The child lease keeps its detached native component alive, but the
        // unreachable parent wrapper (and its local style cache) was collected.
        assert_eq!(dom_plugin.state.lock().unwrap().wrapper_leases.len(), 1);

        let restored: bool = runtime
            .eval(
                r#"
                const recreatedParent = styledChild.parentNode;
                const valuesRestored =
                  recreatedParent.style.getPropertyValue("width") === "41px" &&
                  recreatedParent.style.flexGrow === "2.00";
                recreatedParent.style.flexGrow =
                  String(Number(recreatedParent.style.flexGrow) + 1);
                const removed = recreatedParent.style.removeProperty("width");
                valuesRestored && recreatedParent.style.flexGrow === "3" &&
                  removed === "41px" && recreatedParent.style.width === "";
                "#,
            )
            .await
            .unwrap();

        assert!(restored);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unreachable_wrappers_release_detached_native_subtrees() {
        let (runtime, dom_plugin, _shared) = runtime_with_tracked_dom().await;

        runtime
            .eval::<()>(
                r#"
                (() => {
                  for (let index = 0; index < 250; index++) {
                    const node = document.createElement("div");
                    node.style.width = `${index}px`;
                    node.addEventListener("unused", () => node.className);
                    document.body.appendChild(node);
                    node.remove();
                  }
                })();
                "#,
            )
            .await
            .unwrap();
        collect_garbage(&runtime).await;

        let state = dom_plugin.state.lock().unwrap();
        // Only the permanently connected App and Window remain.
        assert_eq!(state.owner.staging().node_count(), 2);
        assert!(state
            .wrapper_leases
            .keys()
            .all(|id| state.owner.staging().contains(*id)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn connected_nodes_keep_listeners_after_application_references_are_collected() {
        let (runtime, _shared) = runtime_with_dom().await;

        runtime
            .eval::<()>(
                r#"
                globalThis.connectedButtonClicks = 0;
                (() => {
                  const button = document.createElement("div");
                  button.addEventListener("click", () => connectedButtonClicks++);
                  document.body.appendChild(button);
                  globalThis.connectedButtonHandle = button._handle;
                })();
                "#,
            )
            .await
            .unwrap();
        collect_garbage(&runtime).await;

        let handled: bool = runtime
            .eval(
                r#"
                __burokkuDispatchNativeEvent(connectedButtonHandle, { type: "click" });
                connectedButtonClicks === 1;
                "#,
            )
            .await
            .unwrap();
        assert!(handled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_live_descendant_wrapper_retains_its_detached_component() {
        let (runtime, dom_plugin, _shared) = runtime_with_tracked_dom().await;

        runtime
            .eval::<()>(
                r#"
                globalThis.keptContent = (() => {
                  const text = document.createElement("text");
                  const content = document.createTextNode("kept");
                  text.appendChild(content);
                  document.body.appendChild(text);
                  text.remove();
                  return content;
                })();
                "#,
            )
            .await
            .unwrap();
        collect_garbage(&runtime).await;

        assert!(runtime
            .eval::<bool>(
                "keptContent.data === 'kept' && keptContent.parentNode.nodeName === 'TEXT'",
            )
            .await
            .unwrap());
        assert_eq!(
            dom_plugin
                .state
                .lock()
                .unwrap()
                .owner
                .staging()
                .node_count(),
            4
        );

        runtime
            .eval::<()>("delete globalThis.keptContent; void 0")
            .await
            .unwrap();
        collect_garbage(&runtime).await;
        assert_eq!(
            dom_plugin
                .state
                .lock()
                .unwrap()
                .owner
                .staging()
                .node_count(),
            2
        );
    }
}
