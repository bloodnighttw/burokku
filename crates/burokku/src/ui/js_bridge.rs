use std::sync::{Arc, Mutex};

use runtime::{
    rquickjs::{prelude::Func, Ctx, Exception, Object},
    Plugin, Result as RuntimeResult, RuntimeRole,
};
use slotmap::{Key, KeyData};

use super::elements::{BtsDom, Dom, DomError, Elements, NodeId, SharedDom};

const NATIVE_DOM: &str = "__burokkuDomNative";
const DOM_SHIM: &str = include_str!("scripts/dom.js");

#[derive(Debug)]
struct DomState {
    owner: BtsDom,
    body: NodeId,
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
            let body = dom.create(Elements::Window);
            dom.append_child(root, body)
                .expect("a new DOM accepts its initial Window");
            body
        };
        Self {
            state: Arc::new(Mutex::new(DomState { owner, body })),
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
        state_lock(context, &self.state)?
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
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<Option<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &parent_state)?;
                require_element(&context, state.owner.staging(), id)?;
                Ok(state.owner.staging().parent(id).map(encode))
            },
        ),
    )?;

    let first_child_state = state.clone();
    native.set(
        "firstChild",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<Option<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &first_child_state)?;
                let children = require_children(&context, state.owner.staging(), id)?;
                Ok(children.first().copied().map(encode))
            },
        ),
    )?;

    let next_sibling_state = state.clone();
    native.set(
        "nextSibling",
        Func::from(
            move |context: Ctx<'js>, handle: String| -> RuntimeResult<Option<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &next_sibling_state)?;
                let dom = state.owner.staging();
                require_element(&context, dom, id)?;
                let Some(parent) = dom.parent(id) else {
                    return Ok(None);
                };
                let children = require_children(&context, dom, parent)?;
                let index = children
                    .iter()
                    .position(|child| *child == id)
                    .ok_or_else(|| {
                        dom_exception(&context, "node is absent from its parent's children")
                    })?;
                Ok(children.get(index + 1).copied().map(encode))
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
                  -> RuntimeResult<Option<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &attribute_state)?;
                require_element(&context, state.owner.staging(), id)?;
                Ok(state
                    .owner
                    .staging()
                    .attribute(id, &name)
                    .map(str::to_owned))
            },
        ),
    )?;

    let style_state = state.clone();
    native.set(
        "getStyle",
        Func::from(
            move |context: Ctx<'js>,
                  handle: String,
                  name: String|
                  -> RuntimeResult<Option<String>> {
                let id = decode(&context, &handle)?;
                let state = state_lock(&context, &style_state)?;
                require_element(&context, state.owner.staging(), id)?;
                Ok(state.owner.staging().style(id, &name).map(str::to_owned))
            },
        ),
    )?;

    Ok(())
}

fn install_mutations<'js>(native: &Object<'js>, state: &Arc<Mutex<DomState>>) -> RuntimeResult<()> {
    let create_element_state = state.clone();
    native.set(
        "createElement",
        Func::from(
            move |context: Ctx<'js>, tag: String| -> RuntimeResult<String> {
                let element = element_for_tag(&context, &tag)?;
                let mut state = state_lock(&context, &create_element_state)?;
                let id = state.owner.mutate().create(element);
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
                replace_children(&context, &mut state.owner, parent, &children)
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
                set_text_content(&context, &mut state.owner, id, text)
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
                  -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &set_style_state)?;
                let result = state.owner.mutate().set_style(id, name, value);
                map_dom_result(&context, result)
            },
        ),
    )?;

    let remove_style_state = state.clone();
    native.set(
        "removeStyle",
        Func::from(
            move |context: Ctx<'js>, handle: String, name: String| -> RuntimeResult<()> {
                let id = decode(&context, &handle)?;
                let mut state = state_lock(&context, &remove_style_state)?;
                let result = state.owner.mutate().remove_style(id, &name);
                map_dom_result(&context, result).map(drop)
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
        "window" => Ok(Elements::Window),
        "div" => Ok(Elements::Div),
        "flex" => Ok(Elements::Flex {
            style: Box::default(),
        }),
        "grid" => Ok(Elements::Grid {
            style: Box::default(),
        }),
        "text" => Ok(Elements::Text),
        _ => Err(dom_exception(
            context,
            &format!("unsupported element <{tag}>"),
        )),
    }
}

fn element_name(element: &Elements) -> &'static str {
    match element {
        Elements::App => "APP",
        Elements::Window => "WINDOW",
        Elements::Div => "DIV",
        Elements::Flex { .. } => "FLEX",
        Elements::Grid { .. } => "GRID",
        Elements::Text => "TEXT",
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
    if before == Some(child) {
        return Ok(());
    }
    let index = {
        let dom = owner.staging();
        require_element(context, dom, child)?;
        let children = require_children(context, dom, parent)?;
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
    let existing = require_children(context, owner.staging(), parent)?.to_vec();
    for child in existing {
        map_dom_result(context, owner.mutate().detach(child))?;
    }
    for child in children {
        map_dom_result(context, owner.mutate().append_child(parent, *child))?;
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
    if matches!(element, Elements::Text) {
        return map_dom_result(context, owner.mutate().append_child(id, string));
    }

    let text_element = owner.mutate().create(Elements::Text);
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
    async fn attributes_and_styles_are_authoritative_snapshot_data() {
        let (runtime, shared) = runtime_with_dom().await;
        let mut commits = shared.subscribe();

        runtime
            .eval::<()>(
                r#"
                const div = document.createElement("div");
                div.setAttribute("id", "panel");
                div.style.flexGrow = 2;
                div.style.setProperty("background-color", "red");
                document.body.appendChild(div);
                "#,
            )
            .await
            .unwrap();

        commits.changed().await.unwrap();
        let snapshot = shared.load();
        let div = snapshot.dom().children(body(snapshot.dom())).unwrap()[0];
        assert_eq!(snapshot.dom().attribute(div, "id"), Some("panel"));
        assert_eq!(snapshot.dom().style(div, "flex-grow"), Some("2"));
        assert_eq!(snapshot.dom().style(div, "background-color"), Some("red"));
        assert!(snapshot.dom().node(div).unwrap().revisions().style >= 2);
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
            Some(Elements::Div)
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
}
