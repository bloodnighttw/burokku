use std::cell::{Ref, RefMut};

use runtime::rquickjs::{object::Property, prelude::Func, Ctx, Function, Object, Result};

use super::{
    errors,
    lifetime::{decode_node_id, encode_node_id},
    LayoutRect, SharedUiDom, UiDomState,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind};

pub(super) fn install(context: &Ctx<'_>, state: SharedUiDom) -> Result<()> {
    let native = Object::new(context.clone())?;

    native.set(
        "root",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>| -> Result<String> {
                let state = borrow(&context, &state)?;
                Ok(encode_node_id(state.dom.root()))
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
                    let state = borrow(&context, &state)?;
                    state
                        .dom
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.create_element_tag(tag)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.create_text(text)
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
                    let state = borrow(&context, &state)?;
                    state.dom.parent_node(id)
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
                    let state = borrow(&context, &state)?;
                    state
                        .dom
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
                    let state = borrow(&context, &state)?;
                    state.dom.is_connected(id)
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
                    let state = borrow(&context, &state)?;
                    state.dom.contains_node(ancestor, descendant)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.insert_before(parent, child, reference)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.replace_child(parent, new_child, old_child)
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
                    let state = borrow(&context, &state)?;
                    state.dom.text_content(id)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.set_text_content(id, text)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.set_text(id, text)
                };
                errors::map_dom(&context, "set text data", result)
            }
        }),
    )?;

    native.set(
        "getBoundingClientRect",
        Func::from({
            let state = state.clone();
            move |context: Ctx<'_>, token: String, object: Object<'_>| -> Result<bool> {
                let id = decode(&context, &token)?;
                let result = {
                    let state = borrow(&context, &state)?;
                    state.layout_rect(id)
                };
                let Some(rect) = errors::map_dom(&context, "read layout", result)? else {
                    return Ok(false);
                };
                write_layout_rect(&object, rect)?;
                Ok(true)
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
                    let state = borrow(&context, &state)?;
                    state.dom.element_tag(id)
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
                    let state = borrow(&context, &state)?;
                    match state.dom.node(id) {
                        None => Err(DomError::NodeNotFound(id)),
                        Some(node) if node.element().is_none() => Err(DomError::NodeNotElement(id)),
                        Some(_) => Ok(state.dom.attribute(id, &name).map(str::to_owned)),
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
                    let state = borrow(&context, &state)?;
                    match state.dom.node(id) {
                        None => Err(DomError::NodeNotFound(id)),
                        Some(node) if node.element().is_none() => Err(DomError::NodeNotElement(id)),
                        Some(_) => Ok(state.dom.attribute(id, &name).is_some()),
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.set_attribute(id, name, value)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.remove_attribute(id, &name).map(|_| ())
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
                    let state = borrow(&context, &state)?;
                    state.dom.supports_style_property(id, &name)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.set_style_property(id, &name, &value)
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
                    let mut state = borrow_mut(&context, &state)?;
                    state.dom.remove_style_property(id, &name)
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
                    let mut state = borrow_mut(&context, &state)?;
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
                let mut state = borrow_mut(&context, &state)?;
                state.release_wrapper(id);
                Ok(())
            }
        }),
    )?;

    let bootstrap: Function = context.eval(include_str!("../scripts/dom_facade.js"))?;
    bootstrap.call((native,))
}

fn write_layout_rect(object: &Object<'_>, rect: LayoutRect) -> Result<()> {
    for (name, value) in [
        ("x", rect.x),
        ("y", rect.y),
        ("width", rect.width),
        ("height", rect.height),
        ("top", rect.y),
        ("right", rect.x + rect.width),
        ("bottom", rect.y + rect.height),
        ("left", rect.x),
    ] {
        object.prop(name, Property::from(value).enumerable())?;
    }
    Ok(())
}

fn decode(context: &Ctx<'_>, token: &str) -> Result<NodeId> {
    match decode_node_id(token) {
        Ok(id) => Ok(id),
        Err(_) => errors::invalid_token(context),
    }
}

fn borrow<'a>(context: &Ctx<'_>, state: &'a SharedUiDom) -> Result<Ref<'a, UiDomState>> {
    state
        .try_borrow()
        .map_err(|_| errors::borrow_conflict(context))
}

fn borrow_mut<'a>(context: &Ctx<'_>, state: &'a SharedUiDom) -> Result<RefMut<'a, UiDomState>> {
    state
        .try_borrow_mut()
        .map_err(|_| errors::borrow_conflict(context))
}

fn read_optional_node(
    context: &Ctx<'_>,
    state: &SharedUiDom,
    token: &str,
    operation: &str,
    read: impl FnOnce(
        &crate::ui::elements::Dom,
        NodeId,
    ) -> std::result::Result<Option<NodeId>, DomError>,
) -> Result<Option<String>> {
    let id = decode(context, token)?;
    let result = {
        let state = borrow(context, state)?;
        read(&state.dom, id)
    };
    errors::map_dom(context, operation, result).map(|id| id.map(encode_node_id))
}

fn mutate_two_nodes(
    context: &Ctx<'_>,
    state: &SharedUiDom,
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
        let mut state = borrow_mut(context, state)?;
        mutate(&mut state.dom, first, second)
    };
    errors::map_dom(context, operation, result)
}
