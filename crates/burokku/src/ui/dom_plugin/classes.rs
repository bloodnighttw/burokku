use std::{cell::Ref, collections::HashMap, rc::Rc};

use rquickjs::{
    class::Trace, object::Property, prelude::This, CatchResultExt, Class, Coerced, Constructor,
    Ctx, Function, IntoJs, JsLifetime, Null, Object, Result, Value,
};

use super::{
    errors, lifetime::SharedWrapperRoots, LayoutRect, NativeKeyboardEvent, NativeMouseEvent,
    SharedUiDom, UiDomState,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind};

#[derive(Trace, JsLifetime)]
struct WrapperEntry<'js> {
    #[qjs(skip_trace)]
    id: NodeId,
    reference: Object<'js>,
}

#[derive(Trace, JsLifetime)]
struct ListenerRoot<'js> {
    #[qjs(skip_trace)]
    id: NodeId,
    wrapper: Object<'js>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class]
struct WrapperCache<'js> {
    // ponytail: linear lookup; add a traced index only if large DOMs make this measurable.
    entries: Vec<WrapperEntry<'js>>,
    listener_roots: Vec<ListenerRoot<'js>>,
    weak_ref: Constructor<'js>,
    weak_ref_deref: Function<'js>,
}

impl<'js> WrapperCache<'js> {
    fn reference(&self, id: NodeId) -> Option<Object<'js>> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.reference.clone())
    }

    fn remove(&mut self, id: NodeId) {
        self.entries.retain(|entry| entry.id != id);
        self.listener_roots.retain(|entry| entry.id != id);
    }
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "BurokkuMouseEvent", rename_all = "camelCase")]
struct MouseEvent<'js> {
    #[qjs(get, enumerable, rename = "type")]
    event_type: String,
    #[qjs(get, enumerable)]
    target: Object<'js>,
    current_target: Option<Object<'js>>,
    #[qjs(get, enumerable)]
    client_x: f64,
    #[qjs(get, enumerable)]
    client_y: f64,
    #[qjs(get, enumerable)]
    button: u16,
    #[qjs(get, enumerable)]
    buttons: u16,
    #[qjs(get, enumerable)]
    delta_x: f64,
    #[qjs(get, enumerable)]
    delta_y: f64,
    #[qjs(get, enumerable)]
    delta_mode: u16,
    #[qjs(get, enumerable)]
    pointer_id: u32,
    #[qjs(get, enumerable)]
    pointer_type: String,
    #[qjs(get, enumerable)]
    is_primary: bool,
    related_target: Option<Object<'js>>,
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
impl<'js> MouseEvent<'js> {
    #[qjs(get, rename = "relatedTarget", enumerable)]
    fn related_target(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        match &self.related_target {
            Some(target) => Ok(target.clone().into_value()),
            None => Null.into_js(&context),
        }
    }

    #[qjs(get, rename = "currentTarget", enumerable)]
    fn current_target(&self, context: Ctx<'js>) -> Result<Value<'js>> {
        match &self.current_target {
            Some(target) => Ok(target.clone().into_value()),
            None => Null.into_js(&context),
        }
    }

    #[qjs(rename = "preventDefault")]
    fn prevent_default(&mut self) {
        if self.cancelable {
            self.default_prevented = true;
        }
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
    fn current_target(&self, context: Ctx<'js>) -> Result<Value<'js>> {
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

#[derive(Clone, Trace, JsLifetime)]
struct EventListener<'js> {
    id: u64,
    callback: Function<'js>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "NativeNode")]
pub(super) struct NativeNode<'js> {
    #[qjs(skip_trace)]
    state: SharedUiDom,
    #[qjs(skip_trace)]
    id: NodeId,
    #[qjs(skip_trace)]
    wrapper_roots: SharedWrapperRoots,
    listeners: HashMap<String, Vec<EventListener<'js>>>,
    next_listener_id: u64,
}

impl Drop for NativeNode<'_> {
    fn drop(&mut self) {
        self.wrapper_roots.borrow_mut().release(self.id);
    }
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
            .map(|id| wrap_node(&context, &self.state, id))
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
            let event_type = event_type.0;
            if node
                .listeners
                .get(&event_type)
                .is_some_and(|listeners| listeners.iter().any(|item| item.callback == callback))
            {
                return Ok(());
            }
            let id = node.next_listener_id;
            node.next_listener_id = node
                .next_listener_id
                .checked_add(1)
                .expect("event listener IDs exhausted");
            node.listeners
                .entry(event_type)
                .or_default()
                .push(EventListener { id, callback });
        }
        sync_connected_listener_roots(&context, &state)
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
            let event_type = event_type.0;
            let Some(callbacks) = node.listeners.get_mut(&event_type) else {
                return Ok(());
            };
            callbacks.retain(|candidate| candidate.callback != callback);
            if callbacks.is_empty() {
                node.listeners.remove(&event_type);
            }
        }
        sync_connected_listener_roots(&context, &state)
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
        sync_connected_listener_roots(&context, &self.state)?;
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
        sync_connected_listener_roots(&context, &self.state)?;
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
        sync_connected_listener_roots(&context, &self.state)?;
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
        sync_connected_listener_roots(&context, &self.state)?;
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
        sync_connected_listener_roots(&context, &self.state)
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
        wrap_node(&context, &self.state, id)
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
        wrap_node(&context, &self.state, id)
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
        read: impl FnOnce(&UiDomState) -> std::result::Result<Option<NodeId>, DomError>,
    ) -> Result<Value<'js>> {
        let result = {
            let state = borrow(context, &self.state)?;
            read(&state)
        };
        match errors::map_dom(context, operation, result)? {
            Some(id) => Ok(wrap_node(context, &self.state, id)?.into_value()),
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

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "NativeStyleDeclaration", frozen)]
struct NativeStyleDeclaration {
    #[qjs(skip_trace)]
    state: SharedUiDom,
    #[qjs(skip_trace)]
    id: NodeId,
}

#[rquickjs::methods]
impl NativeStyleDeclaration {
    #[qjs(rename = "supportsProperty")]
    fn supports_property(&self, context: Ctx<'_>, name: Coerced<String>) -> Result<bool> {
        let result = borrow(&context, &self.state)?
            .dom
            .supports_style_property(self.id, &name.0);
        errors::map_dom(&context, "check style property", result)
    }

    #[qjs(rename = "setProperty")]
    fn set_property(
        &self,
        context: Ctx<'_>,
        name: Coerced<String>,
        value: Coerced<String>,
    ) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .set_style_property(self.id, &name.0, &value.0);
        errors::map_style(&context, "set style property", result).map(|_| ())
    }

    #[qjs(rename = "removeProperty")]
    fn remove_property(&self, context: Ctx<'_>, name: Coerced<String>) -> Result<()> {
        let result = borrow_mut(&context, &self.state)?
            .dom
            .remove_style_property(self.id, &name.0);
        errors::map_style(&context, "remove style property", result).map(|_| ())
    }
}

const WRAPPER_CACHE: &str = "__burokkuWrapperCache";

pub(super) fn install<'js>(context: &Ctx<'js>, state: SharedUiDom) -> Result<()> {
    let weak_ref: Constructor = context.globals().get("WeakRef")?;
    let weak_ref_deref: Function = weak_ref.get::<_, Object>("prototype")?.get("deref")?;
    let node_methods = Class::<NativeNode<'js>>::prototype(context)?
        .expect("macro-backed Node class has a prototype");
    node_methods.prop(
        WRAPPER_CACHE,
        Class::instance(
            context.clone(),
            WrapperCache {
                entries: Vec::new(),
                listener_roots: Vec::new(),
                weak_ref,
                weak_ref_deref,
            },
        )?,
    )?;
    let style_methods = Class::<NativeStyleDeclaration>::prototype(context)?
        .expect("macro-backed style class has a prototype");
    install_facade(context, &node_methods, &style_methods)?;

    let root = borrow(context, &state)?.dom.root();
    let app = wrap_node(context, &state, root)?;
    context
        .globals()
        .prop("app", Property::from(app).enumerable())?;
    Ok(())
}

fn install_facade<'js>(
    context: &Ctx<'js>,
    node_methods: &Object<'js>,
    style_methods: &Object<'js>,
) -> Result<()> {
    let node = dom_constructor(context, "Node", None)?;
    let app = dom_constructor(context, "AppNode", Some(&node))?;
    let text_node = dom_constructor(context, "TextNode", Some(&node))?;
    let element = dom_constructor(context, "Element", Some(&node))?;
    let window = dom_constructor(context, "Window", Some(&element))?;
    let div = dom_constructor(context, "Div", Some(&element))?;
    let flex = dom_constructor(context, "Flex", Some(&element))?;
    let grid = dom_constructor(context, "Grid", Some(&element))?;
    let text_element = dom_constructor(context, "TextElement", Some(&element))?;
    let style = dom_constructor(context, "BurokkuStyleDeclaration", None)?;

    let node_prototype: Object = node.get("prototype")?;
    copy_properties(
        context,
        &node_prototype,
        node_methods,
        &[
            "parentNode",
            "childNodes",
            "firstChild",
            "lastChild",
            "nextSibling",
            "previousSibling",
            "isConnected",
            "appendChild",
            "insertBefore",
            "removeChild",
            "replaceChild",
            "contains",
            "textContent",
            "nodeValue",
            "addEventListener",
            "removeEventListener",
        ],
    )?;

    copy_properties(
        context,
        &app.get("prototype")?,
        node_methods,
        &["createElement", "createTextNode"],
    )?;
    copy_properties(
        context,
        &text_node.get("prototype")?,
        node_methods,
        &["data"],
    )?;
    copy_properties(
        context,
        &element.get("prototype")?,
        node_methods,
        &[
            "localName",
            "getBoundingClientRect",
            "getAttribute",
            "hasAttribute",
            "setAttribute",
            "removeAttribute",
        ],
    )?;
    copy_properties(
        context,
        &style.get("prototype")?,
        style_methods,
        &["supportsProperty", "setProperty", "removeProperty"],
    )?;

    for (name, constructor) in [
        ("Node", node),
        ("AppNode", app),
        ("TextNode", text_node),
        ("Element", element),
        ("Window", window),
        ("Div", div),
        ("Flex", flex),
        ("Grid", grid),
        ("TextElement", text_element),
        ("BurokkuStyleDeclaration", style),
    ] {
        context.globals().prop(name, constructor)?;
    }
    Ok(())
}

fn dom_constructor<'js>(
    context: &Ctx<'js>,
    name: &str,
    parent: Option<&Constructor<'js>>,
) -> Result<Constructor<'js>> {
    let parent_prototype = parent
        .map(|constructor| constructor.get::<_, Object>("prototype"))
        .transpose()?;
    let prototype = Object::new_proto(context.clone(), parent_prototype.as_ref())?;
    let constructor = Constructor::new_prototype(context, prototype, illegal_constructor)?;
    constructor.set_name(name)?;
    if let Some(parent) = parent {
        let parent: &Object = parent.as_inner().as_inner();
        constructor.set_prototype(Some(parent))?;
    }
    Ok(constructor)
}

fn illegal_constructor<'js>(context: Ctx<'js>) -> Result<Object<'js>> {
    Err(rquickjs::Exception::throw_type(
        &context,
        "Illegal constructor",
    ))
}

fn copy_properties<'js>(
    context: &Ctx<'js>,
    target: &Object<'js>,
    source: &Object<'js>,
    names: &[&str],
) -> Result<()> {
    let object: Object = context.globals().get("Object")?;
    let descriptor: Function = object.get("getOwnPropertyDescriptor")?;
    let define: Function = object.get("defineProperty")?;
    for name in names {
        let property: Object = descriptor.call((source.clone(), *name))?;
        define.call::<_, Object>((target.clone(), *name, property))?;
    }
    Ok(())
}

fn wrapper_cache<'js>(context: &Ctx<'js>) -> Result<Class<'js, WrapperCache<'js>>> {
    Class::<NativeNode<'js>>::prototype(context)?
        .expect("macro-backed Node class has a prototype")
        .get(WRAPPER_CACHE)
}

fn sync_connected_listener_roots<'js>(context: &Ctx<'js>, state: &SharedUiDom) -> Result<()> {
    // ponytail: linear scan; index listener-bearing wrappers only if mutations make this measurable.
    // ponytail: detached descendants survive only while their wrappers are live; root detached
    // component groups if browser-compatible subtree retention becomes necessary.
    let cache = wrapper_cache(context)?;
    let (candidates, deref) = {
        let cache = cache.borrow();
        (
            cache
                .entries
                .iter()
                .map(|entry| (entry.id, entry.reference.clone()))
                .collect::<Vec<_>>(),
            cache.weak_ref_deref.clone(),
        )
    };
    let candidates: Vec<_> = {
        let state = borrow(context, state)?;
        candidates
            .into_iter()
            .filter(|(id, _)| matches!(state.dom.is_connected(*id), Ok(true)))
            .collect()
    };

    let mut roots = Vec::new();
    for (id, reference) in candidates {
        let Some(wrapper) = deref.call::<_, Option<Object>>((This(reference),))? else {
            continue;
        };
        let node =
            Class::<NativeNode>::from_object(&wrapper).expect("wrapped nodes use NativeNode");
        if !node.borrow().listeners.is_empty() {
            roots.push(ListenerRoot { id, wrapper });
        }
    }
    cache.borrow_mut().listener_roots = roots;
    Ok(())
}

fn cached_wrapper<'js>(
    cache: &Class<'js, WrapperCache<'js>>,
    id: NodeId,
) -> Result<Option<Object<'js>>> {
    let (reference, deref) = {
        let cache = cache.borrow();
        (cache.reference(id), cache.weak_ref_deref.clone())
    };
    let Some(reference) = reference else {
        return Ok(None);
    };
    deref.call((This(reference),))
}

fn cache_wrapper<'js>(
    cache: &Class<'js, WrapperCache<'js>>,
    id: NodeId,
    wrapper: &Object<'js>,
) -> Result<()> {
    let weak_ref = cache.borrow().weak_ref.clone();
    let reference = weak_ref.construct((wrapper.clone(),))?;
    cache
        .borrow_mut()
        .entries
        .push(WrapperEntry { id, reference });
    Ok(())
}

fn wrap_node<'js>(context: &Ctx<'js>, state: &SharedUiDom, id: NodeId) -> Result<Object<'js>> {
    let cache = wrapper_cache(context)?;
    let released = borrow(context, state)?.take_released_wrappers();
    if !released.is_empty() {
        let mut cache = cache.borrow_mut();
        for id in released {
            cache.remove(id);
        }
    }
    if let Some(cached) = cached_wrapper(&cache, id)? {
        return Ok(cached);
    }

    let (constructor_name, is_element) = {
        let state = borrow(context, state)?;
        let kind = state.dom.kind(id).ok_or(DomError::NodeNotFound(id));
        let kind = errors::map_dom(context, "wrap node", kind)?;
        match kind {
            NodeKind::App => ("AppNode", false),
            NodeKind::Text(_) => ("TextNode", false),
            NodeKind::Element(element) => match element.tag() {
                ElementTag::Window => ("Window", true),
                ElementTag::Div => ("Div", true),
                ElementTag::Flex => ("Flex", true),
                ElementTag::Grid => ("Grid", true),
                ElementTag::Text => ("TextElement", true),
            },
        }
    };
    let constructor: Object = context.globals().get(constructor_name)?;
    let prototype: Object = constructor.get("prototype")?;
    let style_prototype = if is_element {
        let constructor: Object = context.globals().get("BurokkuStyleDeclaration")?;
        Some(constructor.get("prototype")?)
    } else {
        None
    };
    let wrapper_roots = {
        let state = borrow(context, state)?;
        errors::map_dom(context, "acquire node wrapper", state.acquire_wrapper(id))?
    };
    let node = Class::instance_proto(
        NativeNode {
            state: state.clone(),
            id,
            wrapper_roots,
            listeners: HashMap::new(),
            next_listener_id: 0,
        },
        prototype,
    )?;

    if let Some(prototype) = style_prototype {
        let style = Class::instance_proto(
            NativeStyleDeclaration {
                state: state.clone(),
                id,
            },
            prototype,
        )?;
        node.prop("style", Property::from(style))?;
    }

    cache_wrapper(
        &cache,
        id,
        node.as_value()
            .as_object()
            .expect("class instance is an object"),
    )?;
    Ok(node.into_inner())
}

pub(super) fn dispatch_mouse_event(context: &Ctx<'_>, mouse: NativeMouseEvent) -> Result<()> {
    let app: Object = context.globals().get("app")?;
    let Some(app) = Class::<NativeNode>::from_object(&app) else {
        return Ok(());
    };
    let state = app.borrow().state.clone();
    let bubbles = !matches!(mouse.event_type, "mouseenter" | "mouseleave");
    let (path, related_target) = {
        let state = borrow(context, &state)?;
        debug_assert!(mouse.presented_revision <= state.dom.revision());
        if !state.dom.is_connected(mouse.target).unwrap_or(false) {
            return Ok(());
        }

        let mut path = Vec::new();
        let mut current = Some(mouse.target);
        while let Some(id) = current {
            path.push(id);
            if !bubbles {
                break;
            }
            current = errors::map_dom(
                context,
                "build mouse propagation path",
                state.dom.parent_node(id),
            )?;
        }
        (
            path,
            mouse
                .related_target
                .filter(|id| state.dom.node(*id).is_some()),
        )
    };

    let mut listeners = Vec::with_capacity(path.len());
    for id in path {
        let current = wrap_node(context, &state, id)?;
        let node =
            Class::<NativeNode>::from_object(&current).expect("wrapped nodes use NativeNode");
        let callbacks = node
            .borrow()
            .listeners
            .get(mouse.event_type)
            .cloned()
            .unwrap_or_default();
        listeners.push((current, callbacks));
    }
    if listeners.iter().all(|(_, callbacks)| callbacks.is_empty()) {
        return Ok(());
    }

    let target = listeners[0].0.clone();
    let related_target = related_target
        .map(|id| wrap_node(context, &state, id))
        .transpose()?;
    let (delta_x, delta_y, delta_mode) = mouse.wheel_delta.unwrap_or((0.0, 0.0, 0));
    let pointer_id = mouse.pointer_id.unwrap_or(0);
    let event = Class::instance(
        context.clone(),
        MouseEvent {
            event_type: mouse.event_type.into(),
            target,
            current_target: None,
            client_x: mouse.client_x,
            client_y: mouse.client_y,
            button: mouse.button,
            buttons: mouse.buttons,
            delta_x,
            delta_y,
            delta_mode,
            pointer_id,
            pointer_type: if pointer_id == 0 { "" } else { "mouse" }.into(),
            is_primary: pointer_id != 0,
            related_target,
            bubbles,
            cancelable: bubbles,
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
                .get(mouse.event_type)
                .is_some_and(|listeners| listeners.iter().any(|item| item.id == listener.id));
            if !still_registered {
                continue;
            }
            if let Err(error) = listener
                .callback
                .call::<_, ()>((This(current.clone()), event.clone()))
                .catch(context)
            {
                eprintln!("Burokku {} listener failed: {error}", mouse.event_type);
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

pub(super) fn dispatch_keyboard_event(
    context: &Ctx<'_>,
    keyboard: NativeKeyboardEvent,
) -> Result<()> {
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
        object.prop(name, Property::from(value).enumerable())?;
    }
    Ok(object)
}

fn borrow<'a>(context: &Ctx<'_>, state: &'a SharedUiDom) -> Result<Ref<'a, UiDomState>> {
    state
        .try_borrow()
        .map_err(|_| errors::borrow_conflict(context))
}

fn borrow_mut<'a>(
    context: &Ctx<'_>,
    state: &'a SharedUiDom,
) -> Result<std::cell::RefMut<'a, UiDomState>> {
    state
        .try_borrow_mut()
        .map_err(|_| errors::borrow_conflict(context))
}
