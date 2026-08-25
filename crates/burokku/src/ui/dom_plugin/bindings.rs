use std::sync::MutexGuard;

use runtime::rquickjs::{prelude::Func, Ctx, Function, Object, Result};

use super::{
    errors,
    lifetime::{decode_node_id, encode_node_id},
    DomPluginState, SharedDomState,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind};

pub(super) fn install(context: &Ctx<'_>, state: SharedDomState) -> Result<()> {
    let native = Object::new(context.clone())?;

    native.set(
        "root",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>| -> Result<String> {
                let state = lock(&context, &state)?;
                Ok(encode_node_id(state.staging.root()))
            }
        }),
    )?;
    native.set(
        "kind",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<String> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state
                        .staging
                        .kind(id)
                        .map(|kind| match kind {
                            NodeKind::App => "app",
                            NodeKind::Text(_) => "text",
                            NodeKind::Element(element) => match element.tag() {
                                ElementTag::Window => "window",
                                ElementTag::Div => "div",
                                ElementTag::Flex => "flex",
                                ElementTag::Grid => "grid",
                                ElementTag::Text => "text-element",
                            },
                        })
                        .ok_or(DomError::NodeNotFound(id))
                };
                errors::map_dom(&context, "read node kind", result).map(str::to_owned)
            }
        }),
    )?;
    native.set(
        "createElement",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, name: String| -> Result<String> {
                let tag = match ElementTag::try_from(name.as_str()) {
                    Ok(tag) => tag,
                    Err(error) => return errors::invalid_tag(&context, error),
                };
                let id = {
                    let mut state = lock(&context, &state)?;
                    state.staging.create_element_tag(tag)
                };
                Ok(encode_node_id(id))
            }
        }),
    )?;
    native.set(
        "createText",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, text: String| -> Result<String> {
                let id = {
                    let mut state = lock(&context, &state)?;
                    state.staging.create_text(text)
                };
                Ok(encode_node_id(id))
            }
        }),
    )?;

    native.set(
        "parent",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<Option<String>> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state.staging.parent_node(id)
                };
                errors::map_dom(&context, "read parentNode", result)
                    .map(|parent| parent.map(encode_node_id))
            }
        }),
    )?;
    native.set(
        "children",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<Vec<String>> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state
                        .staging
                        .children(id)
                        .map(|children| children.to_vec())
                        .ok_or(DomError::NodeNotFound(id))
                };
                errors::map_dom(&context, "read childNodes", result)
                    .map(|children| children.into_iter().map(encode_node_id).collect())
            }
        }),
    )?;
    native.set(
        "firstChild",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<Option<String>> {
                read_optional_node(&context, &state, &token, "read firstChild", |dom, id| {
                    dom.first_child(id)
                })
            }
        }),
    )?;
    native.set(
        "lastChild",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<Option<String>> {
                read_optional_node(&context, &state, &token, "read lastChild", |dom, id| {
                    dom.last_child(id)
                })
            }
        }),
    )?;
    native.set(
        "nextSibling",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<Option<String>> {
                read_optional_node(&context, &state, &token, "read nextSibling", |dom, id| {
                    dom.next_sibling(id)
                })
            }
        }),
    )?;
    native.set(
        "previousSibling",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<Option<String>> {
                read_optional_node(
                    &context,
                    &state,
                    &token,
                    "read previousSibling",
                    |dom, id| dom.previous_sibling(id),
                )
            }
        }),
    )?;
    native.set(
        "isConnected",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state.staging.is_connected(id)
                };
                errors::map_dom(&context, "read isConnected", result)
            }
        }),
    )?;
    native.set(
        "contains",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, ancestor: String, descendant: String| -> Result<bool> {
                let ancestor = decode(&context, &ancestor)?;
                let descendant = decode(&context, &descendant)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state.staging.contains_node(ancestor, descendant)
                };
                errors::map_dom(&context, "check node containment", result)
            }
        }),
    )?;

    native.set(
        "appendChild",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, parent: String, child: String| -> Result<()> {
                mutate_two_nodes(
                    &context,
                    &state,
                    &parent,
                    &child,
                    "appendChild",
                    |dom, p, c| dom.append_child(p, c),
                )
            }
        }),
    )?;
    native.set(
        "insertBefore",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>,
                  parent: String,
                  child: String,
                  reference: Option<String>|
                  -> Result<()> {
                let parent = decode(&context, &parent)?;
                let child = decode(&context, &child)?;
                let reference = reference
                    .as_deref()
                    .map(|token| decode(&context, token))
                    .transpose()?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.insert_before(parent, child, reference)
                };
                errors::map_dom(&context, "insertBefore", result)
            }
        }),
    )?;
    native.set(
        "removeChild",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, parent: String, child: String| -> Result<()> {
                mutate_two_nodes(
                    &context,
                    &state,
                    &parent,
                    &child,
                    "removeChild",
                    |dom, p, c| dom.remove_child(p, c),
                )
            }
        }),
    )?;
    native.set(
        "replaceChild",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>,
                  parent: String,
                  new_child: String,
                  old_child: String|
                  -> Result<()> {
                let parent = decode(&context, &parent)?;
                let new_child = decode(&context, &new_child)?;
                let old_child = decode(&context, &old_child)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.replace_child(parent, new_child, old_child)
                };
                errors::map_dom(&context, "replaceChild", result)
            }
        }),
    )?;

    native.set(
        "textContent",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<String> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state.staging.text_content(id)
                };
                errors::map_dom(&context, "read textContent", result)
            }
        }),
    )?;
    native.set(
        "setTextContent",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, text: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.set_text_content(id, text)
                };
                errors::map_dom(&context, "set textContent", result)
            }
        }),
    )?;
    native.set(
        "setText",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, text: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.set_text(id, text)
                };
                errors::map_dom(&context, "set text data", result)
            }
        }),
    )?;

    native.set(
        "localName",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<String> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state.staging.element_tag(id)
                };
                errors::map_dom(&context, "read localName", result)
                    .map(|tag| tag.local_name().to_owned())
            }
        }),
    )?;
    native.set(
        "getAttribute",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String| -> Result<Option<String>> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    match state.staging.node(id) {
                        None => Err(DomError::NodeNotFound(id)),
                        Some(node) if node.element().is_none() => Err(DomError::NodeNotElement(id)),
                        Some(_) => Ok(state.staging.attribute(id, &name).map(str::to_owned)),
                    }
                };
                errors::map_dom(&context, "getAttribute", result)
            }
        }),
    )?;
    native.set(
        "hasAttribute",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    match state.staging.node(id) {
                        None => Err(DomError::NodeNotFound(id)),
                        Some(node) if node.element().is_none() => Err(DomError::NodeNotElement(id)),
                        Some(_) => Ok(state.staging.attribute(id, &name).is_some()),
                    }
                };
                errors::map_dom(&context, "hasAttribute", result)
            }
        }),
    )?;
    native.set(
        "setAttribute",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String, value: String| -> Result<()> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.set_attribute(id, name, value)
                };
                errors::map_dom(&context, "setAttribute", result)
            }
        }),
    )?;
    native.set(
        "removeAttribute",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String| -> Result<()> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.remove_attribute(id, &name).map(|_| ())
                };
                errors::map_dom(&context, "removeAttribute", result)
            }
        }),
    )?;

    native.set(
        "supportsStyleProperty",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = lock(&context, &state)?;
                    state.staging.supports_style_property(id, &name)
                };
                errors::map_dom(&context, "check style property", result)
            }
        }),
    )?;
    native.set(
        "setStyleProperty",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String, value: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.set_style_property(id, &name, &value)
                };
                errors::map_style(&context, "set style property", result)
            }
        }),
    )?;
    native.set(
        "removeStyleProperty",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, name: String| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.staging.remove_style_property(id, &name)
                };
                errors::map_style(&context, "remove style property", result)
            }
        }),
    )?;

    native.set(
        "acquireWrapper",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<()> {
                let id = decode(&context, &token)?;
                let result = {
                    let mut state = lock(&context, &state)?;
                    state.acquire_wrapper(id)
                };
                errors::map_dom(&context, "acquire node wrapper", result)
            }
        }),
    )?;
    native.set(
        "releaseWrapper",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String| -> Result<()> {
                let id = decode(&context, &token)?;
                let mut state = lock(&context, &state)?;
                state.release_wrapper(id);
                Ok(())
            }
        }),
    )?;

    let bootstrap: Function = context.eval(include_str!("../scripts/dom_facade.js"))?;
    bootstrap.call((native,))
}

fn decode(context: &Ctx<'_>, token: &str) -> Result<NodeId> {
    match decode_node_id(token) {
        Ok(id) => Ok(id),
        Err(_) => errors::invalid_token(context),
    }
}

fn lock<'a>(
    context: &Ctx<'_>,
    state: &'a SharedDomState,
) -> Result<MutexGuard<'a, DomPluginState>> {
    match state.lock() {
        Ok(state) => Ok(state),
        Err(_) => errors::poisoned(context),
    }
}

fn read_optional_node(
    context: &Ctx<'_>,
    state: &SharedDomState,
    token: &str,
    operation: &str,
    read: impl FnOnce(
        &crate::ui::elements::Dom,
        NodeId,
    ) -> std::result::Result<Option<NodeId>, DomError>,
) -> Result<Option<String>> {
    let id = decode(context, token)?;
    let result = {
        let state = lock(context, state)?;
        read(&state.staging, id)
    };
    errors::map_dom(context, operation, result).map(|id| id.map(encode_node_id))
}

fn mutate_two_nodes(
    context: &Ctx<'_>,
    state: &SharedDomState,
    first: &str,
    second: &str,
    operation: &str,
    mutate: impl FnOnce(
        &mut crate::ui::elements::Dom,
        NodeId,
        NodeId,
    ) -> std::result::Result<(), DomError>,
) -> Result<()> {
    let first = decode(context, first)?;
    let second = decode(context, second)?;
    let result = {
        let mut state = lock(context, state)?;
        mutate(&mut state.staging, first, second)
    };
    errors::map_dom(context, operation, result)
}
