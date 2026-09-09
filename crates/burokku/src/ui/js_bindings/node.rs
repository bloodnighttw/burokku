//! JavaScript-facing DOM node bindings.

use std::rc::Rc;

use rquickjs::{
    class::Trace, prelude::This, Class, Coerced, Ctx, Function, IntoJs, JsLifetime, Null, Object,
    Result, Value,
};

use super::{
    borrow, borrow_mut, errors,
    events::listeners::ListenerRegistry,
    wrapper::{self, WrapperRoot},
    DomBindingState, LayoutRect, SharedDomBindings,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind};

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "NativeNode")]
pub(super) struct NativeNode<'js> {
    #[qjs(skip_trace)]
    pub(super) state: SharedDomBindings,
    #[qjs(skip_trace)]
    pub(super) id: NodeId,
    #[qjs(skip_trace)]
    pub(super) _wrapper_root: WrapperRoot,
    pub(super) listeners: ListenerRegistry<'js>,
}
#[rquickjs::methods]
impl<'js> NativeNode<'js> {
    #[qjs(get, rename = "parentNode")]
    fn parent_node(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        self.optional_node(&context, "read parentNode", |state| {
            state.dom.parent_node(self.id)
        })
    }

    #[qjs(get, rename = "childNodes")]
    fn child_nodes(&self, context: Ctx<'js>) -> Result<Vec<Object<'js>>> {
        let children = {
            let state = borrow(&context, &self.state)?;
            state
                .dom
                .children(self.id)
                .map(|children| children.to_vec())
                .ok_or(DomError::NodeNotFound(self.id))
        };
        errors::map_dom(&context, "read childNodes", children)?
            .into_iter()
            .map(|id| wrapper::wrap_node(&context, &self.state, id))
            .collect()
    }

    #[qjs(get, rename = "firstChild")]
    fn first_child(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        self.optional_node(&context, "read firstChild", |state| {
            state.dom.first_child(self.id)
        })
    }

    #[qjs(get, rename = "lastChild")]
    fn last_child(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        self.optional_node(&context, "read lastChild", |state| {
            state.dom.last_child(self.id)
        })
    }

    #[qjs(get, rename = "nextSibling")]
    fn next_sibling(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        self.optional_node(&context, "read nextSibling", |state| {
            state.dom.next_sibling(self.id)
        })
    }

    #[qjs(get, rename = "previousSibling")]
    fn previous_sibling(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        self.optional_node(&context, "read previousSibling", |state| {
            state.dom.previous_sibling(self.id)
        })
    }

    #[qjs(get, rename = "isConnected")]
    fn is_connected(&self, context: Ctx<'js>) -> Result<bool> {
        let result = borrow(&context, &self.state)?.dom.is_connected(self.id);
        errors::map_dom(&context, "read isConnected", result)
    }

    #[qjs(rename = "addEventListener")]
    fn add_event_listener(
        this: This<Class<'js, NativeNode<'js>>>,
        context: Ctx<'js>,
        event_type: Coerced<String>,
        callback: Function<'js>,
    ) -> Result<()> {
        let state = this.0.borrow().state.clone();
        {
            let mut node = this.0.borrow_mut();
            if !node.listeners.add(event_type.0, callback) {
                return Ok(());
            }
        }
        wrapper::sync_connected_listener_roots(&context, &state)
    }

    #[qjs(rename = "removeEventListener")]
    fn remove_event_listener(
        this: This<Class<'js, NativeNode<'js>>>,
        context: Ctx<'js>,
        event_type: Coerced<String>,
        callback: Value<'js>,
    ) -> Result<()> {
        let Some(callback) = callback.into_function() else {
            return Ok(());
        };
        let state = this.0.borrow().state.clone();
        {
            let mut node = this.0.borrow_mut();
            if !node.listeners.remove(&event_type.0, &callback) {
                return Ok(());
            }
        }
        wrapper::sync_connected_listener_roots(&context, &state)
    }

    #[qjs(rename = "setPointerCapture")]
    fn set_pointer_capture(&self, context: Ctx<'js>, pointer_id: u32) -> Result<()> {
        let mut state = borrow_mut(&context, &self.state)?;
        if pointer_id != 1 || state.pointer.active.is_none() {
            return errors::throw_named(&context, "NotFoundError", "pointer is not active");
        }
        if !state.dom.is_connected(self.id).unwrap_or(false) {
            return errors::throw_named(
                &context,
                "InvalidStateError",
                "capture target is not connected",
            );
        }
        state.pointer.capture = Some(self.id);
        Ok(())
    }

    #[qjs(rename = "releasePointerCapture")]
    fn release_pointer_capture(&self, context: Ctx<'js>, pointer_id: u32) -> Result<()> {
        let mut state = borrow_mut(&context, &self.state)?;
        if pointer_id != 1 || state.pointer.active.is_none() {
            return errors::throw_named(&context, "NotFoundError", "pointer is not active");
        }
        if state.pointer.capture == Some(self.id) {
            state.pointer.capture = None;
        }
        Ok(())
    }

    #[qjs(rename = "hasPointerCapture")]
    fn has_pointer_capture(&self, context: Ctx<'js>, pointer_id: u32) -> Result<bool> {
        let mut state = borrow_mut(&context, &self.state)?;
        state.clear_disconnected_pointer_capture();
        Ok(pointer_id == 1 && state.pointer.capture == Some(self.id))
    }

    #[qjs(rename = "appendChild")]
    fn append_child(
        &self,
        context: Ctx<'js>,
        child: Class<'js, NativeNode<'js>>,
    ) -> Result<Class<'js, NativeNode<'js>>> {
        let child_id = self.related_id(&context, &child)?;
        let result = borrow_mut(&context, &self.state)?
            .dom
            .append_child(self.id, child_id);
        errors::map_dom(&context, "appendChild", result)?;
        wrapper::sync_connected_listener_roots(&context, &self.state)?;
        Ok(child)
    }

    #[qjs(rename = "insertBefore")]
    fn insert_before(
        &self,
        context: Ctx<'js>,
        child: Class<'js, NativeNode<'js>>,
        reference: Option<Class<'js, NativeNode<'js>>>,
    ) -> Result<Class<'js, NativeNode<'js>>> {
        let child_id = self.related_id(&context, &child)?;
        let reference_id = reference
            .as_ref()
            .map(|node| self.related_id(&context, node))
            .transpose()?;
        let result =
            borrow_mut(&context, &self.state)?
                .dom
                .insert_before(self.id, child_id, reference_id);
        errors::map_dom(&context, "insertBefore", result)?;
        wrapper::sync_connected_listener_roots(&context, &self.state)?;
        Ok(child)
    }

    #[qjs(rename = "removeChild")]
    fn remove_child(
        &self,
        context: Ctx<'js>,
        child: Class<'js, NativeNode<'js>>,
    ) -> Result<Class<'js, NativeNode<'js>>> {
        let child_id = self.related_id(&context, &child)?;
        let result = borrow_mut(&context, &self.state)?
            .dom
            .remove_child(self.id, child_id);
        errors::map_dom(&context, "removeChild", result)?;
        wrapper::sync_connected_listener_roots(&context, &self.state)?;
        Ok(child)
    }

    #[qjs(rename = "replaceChild")]
    fn replace_child(
        &self,
        context: Ctx<'js>,
        new_child: Class<'js, NativeNode<'js>>,
        old_child: Class<'js, NativeNode<'js>>,
    ) -> Result<Class<'js, NativeNode<'js>>> {
        let new_id = self.related_id(&context, &new_child)?;
        let old_id = self.related_id(&context, &old_child)?;
        let result = borrow_mut(&context, &self.state)?
            .dom
            .replace_child(self.id, new_id, old_id);
        errors::map_dom(&context, "replaceChild", result)?;
        wrapper::sync_connected_listener_roots(&context, &self.state)?;
        Ok(old_child)
    }

    fn contains(&self, context: Ctx<'js>, other: Class<'js, NativeNode<'js>>) -> Result<bool> {
        let other = self.related_id(&context, &other)?;
        let result = borrow(&context, &self.state)?
            .dom
            .contains_node(self.id, other);
        errors::map_dom(&context, "check node containment", result)
    }

    #[qjs(get, rename = "textContent")]
    fn get_text_content(&self, context: Ctx<'js>) -> Result<String> {
        let result = borrow(&context, &self.state)?.dom.text_content(self.id);
        errors::map_dom(&context, "read textContent", result)
    }

    #[qjs(set, rename = "textContent")]
    fn set_text_content(&self, context: Ctx<'js>, text: Coerced<String>) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .set_text_content(self.id, text.0);
        errors::map_dom(&context, "set textContent", result)?;
        wrapper::sync_connected_listener_roots(&context, &self.state)
    }

    #[qjs(get, rename = "nodeValue")]
    fn get_node_value(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        let state = borrow(&context, &self.state)?;
        match state.dom.kind(self.id) {
            Some(NodeKind::Text(text)) => text.to_owned().into_js(&context),
            Some(_) => Null.into_js(&context),
            None => errors::map_dom(
                &context,
                "read nodeValue",
                Err::<(), _>(DomError::NodeNotFound(self.id)),
            )
            .and_then(|_| Null.into_js(&context)),
        }
    }

    #[qjs(set, rename = "nodeValue")]
    fn set_node_value(&self, context: Ctx<'js>, text: Coerced<String>) -> Result<()> {
        let is_text = {
            let state = borrow(&context, &self.state)?;
            match state.dom.kind(self.id) {
                Some(NodeKind::Text(_)) => true,
                Some(_) => false,
                None => {
                    return errors::map_dom(
                        &context,
                        "set nodeValue",
                        Err(DomError::NodeNotFound(self.id)),
                    )
                }
            }
        };
        if is_text {
            let result = borrow_mut(&context, &self.state)?
                .dom
                .set_text(self.id, text.0);
            errors::map_dom(&context, "set nodeValue", result)?;
        }
        Ok(())
    }

    #[qjs(rename = "createElement")]
    fn create_element(&self, context: Ctx<'js>, name: Coerced<String>) -> Result<Object<'js>> {
        let tag = match ElementTag::try_from(name.0.as_str()) {
            Ok(tag) => tag,
            Err(error) => return errors::invalid_tag(&context, error),
        };
        let id = {
            let mut state = borrow_mut(&context, &self.state)?;
            if self.id != state.dom.root() {
                return Err(rquickjs::Exception::throw_type(
                    &context,
                    "createElement is only available on app",
                ));
            }
            state.dom.create_element_tag(tag)
        };
        wrapper::wrap_node(&context, &self.state, id)
    }

    #[qjs(rename = "createTextNode")]
    fn create_text_node(&self, context: Ctx<'js>, text: Coerced<String>) -> Result<Object<'js>> {
        let id = {
            let mut state = borrow_mut(&context, &self.state)?;
            if self.id != state.dom.root() {
                return Err(rquickjs::Exception::throw_type(
                    &context,
                    "createTextNode is only available on app",
                ));
            }
            state.dom.create_text(text.0)
        };
        wrapper::wrap_node(&context, &self.state, id)
    }

    #[qjs(get, rename = "data")]
    fn get_data(&self, context: Ctx<'js>) -> Result<String> {
        let result = {
            let state = borrow(&context, &self.state)?;
            match state.dom.kind(self.id) {
                Some(NodeKind::Text(text)) => Ok(text.to_owned()),
                Some(_) => Err(DomError::NodeNotText(self.id)),
                None => Err(DomError::NodeNotFound(self.id)),
            }
        };
        errors::map_dom(&context, "read text data", result)
    }

    #[qjs(set, rename = "data")]
    fn set_data(&self, context: Ctx<'js>, text: Coerced<String>) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .set_text(self.id, text.0);
        errors::map_dom(&context, "set text data", result).map(|_| ())
    }

    #[qjs(get, rename = "localName")]
    fn local_name(&self, context: Ctx<'js>) -> Result<String> {
        let result = borrow(&context, &self.state)?.dom.element_tag(self.id);
        errors::map_dom(&context, "read localName", result).map(|tag| tag.local_name().to_owned())
    }

    #[qjs(rename = "getBoundingClientRect")]
    fn get_bounding_client_rect(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        let rect = {
            let state = borrow(&context, &self.state)?;
            state.layout_rect(self.id)
        };
        let Some(rect) = errors::map_dom(&context, "read layout", rect)? else {
            return Null.into_js(&context);
        };
        let object = layout_rect_object(&context, rect)?;
        let object_constructor: Object = context.globals().get("Object")?;
        let freeze: Function = object_constructor.get("freeze")?;
        freeze.call::<_, Object>((object,)).map(Object::into_value)
    }

    #[qjs(rename = "getAttribute")]
    fn get_attribute(&self, context: Ctx<'js>, name: Coerced<String>) -> Result<Value<'js>> {
        let result = {
            let state = borrow(&context, &self.state)?;
            match state.dom.node(self.id) {
                None => Err(DomError::NodeNotFound(self.id)),
                Some(node) if node.element().is_none() => Err(DomError::NodeNotElement(self.id)),
                Some(_) => Ok(state.dom.attribute(self.id, &name.0).map(str::to_owned)),
            }
        };
        match errors::map_dom(&context, "getAttribute", result)? {
            Some(value) => value.into_js(&context),
            None => Null.into_js(&context),
        }
    }

    #[qjs(rename = "hasAttribute")]
    fn has_attribute(&self, context: Ctx<'js>, name: Coerced<String>) -> Result<bool> {
        let result = {
            let state = borrow(&context, &self.state)?;
            match state.dom.node(self.id) {
                None => Err(DomError::NodeNotFound(self.id)),
                Some(node) if node.element().is_none() => Err(DomError::NodeNotElement(self.id)),
                Some(_) => Ok(state.dom.attribute(self.id, &name.0).is_some()),
            }
        };
        errors::map_dom(&context, "hasAttribute", result)
    }

    #[qjs(rename = "setAttribute")]
    fn set_attribute(
        &self,
        context: Ctx<'js>,
        name: Coerced<String>,
        value: Coerced<String>,
    ) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .set_attribute(self.id, name.0, value.0);
        errors::map_dom(&context, "setAttribute", result)
    }

    #[qjs(rename = "removeAttribute")]
    fn remove_attribute(&self, context: Ctx<'js>, name: Coerced<String>) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .remove_attribute(self.id, &name.0)
            .map(|_| ());
        errors::map_dom(&context, "removeAttribute", result)
    }

    #[qjs(skip)]
    fn optional_node(
        &self,
        context: &Ctx<'js>,
        operation: &str,
        read: impl FnOnce(&DomBindingState) -> std::result::Result<Option<NodeId>, DomError>,
    ) -> Result<Value<'js>> {
        let result = {
            let state = borrow(context, &self.state)?;
            read(&state)
        };
        match errors::map_dom(context, operation, result)? {
            Some(id) => Ok(wrapper::wrap_node(context, &self.state, id)?.into_value()),
            None => Null.into_js(context),
        }
    }

    #[qjs(skip)]
    fn related_id(&self, context: &Ctx<'js>, node: &Class<'js, NativeNode<'js>>) -> Result<NodeId> {
        let node = node.try_borrow()?;
        if !Rc::ptr_eq(&self.state, &node.state) {
            return errors::invalid_token(context);
        }
        Ok(node.id)
    }
}

fn layout_rect_object<'js>(context: &Ctx<'js>, rect: LayoutRect) -> Result<Object<'js>> {
    let object = Object::new(context.clone())?;
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
        object.prop(name, rquickjs::object::Property::from(value).enumerable())?;
    }
    Ok(object)
}
