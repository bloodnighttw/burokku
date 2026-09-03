use std::{cell::Ref, collections::HashMap, rc::Rc};

use rquickjs::{
    class::Trace, object::Property, Class, Coerced, Constructor, Ctx, Function, IntoJs,
    JsLifetime, Null, Object, Result, Value,
};

use super::{
    errors,
    lifetime::SharedWrapperRoots,
    LayoutRect, SharedUiDom, UiDomState,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind};

#[derive(Trace, JsLifetime)]
struct WrapperEntry<'js> {
    #[qjs(skip_trace)]
    id: NodeId,
    reference: Object<'js>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class(frozen)]
struct WrapperCache<'js> {
    // ponytail: linear lookup; add a traced index only if large DOMs make this measurable.
    entries: Vec<WrapperEntry<'js>>,
}

impl WrapperCache<'_> {
    fn reference(&self, id: NodeId) -> Option<Object<'_>> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.reference.clone())
    }

    fn remove(&mut self, id: NodeId) {
        self.entries.retain(|entry| entry.id != id);
    }
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
    cache: Class<'js, WrapperCache<'js>>,
    listeners: HashMap<String, Vec<Function<'js>>>,
}

impl Drop for NativeNode<'_> {
    fn drop(&mut self) {
        self.cache.borrow_mut().remove(self.id);
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
    fn add_event_listener(&mut self, event_type: Coerced<String>, callback: Function<'js>) {
        let callbacks = self.listeners.entry(event_type.0).or_default();
        if !callbacks.contains(&callback) {
            callbacks.push(callback);
        }
    }

    #[qjs(rename = "removeEventListener")]
    fn remove_event_listener(&mut self, event_type: Coerced<String>, callback: Value<'js>) {
        let Some(callback) = callback.into_function() else {
            return;
        };
        let Some(callbacks) = self.listeners.get_mut(&event_type.0) else {
            return;
        };
        callbacks.retain(|candidate| candidate != &callback);
        if callbacks.is_empty() {
            self.listeners.remove(&event_type.0);
        }
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
        Ok(old_child)
    }

    fn contains(
        &self,
        context: Ctx<'js>,
        other: Class<'js, NativeNode<'js>>,
    ) -> Result<bool> {
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
        errors::map_dom(&context, "set textContent", result).map(|_| ())
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
    fn related_id(
        &self,
        context: &Ctx<'js>,
        node: &Class<'js, NativeNode<'js>>,
    ) -> Result<NodeId> {
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
    let node_methods = Class::<NativeNode<'js>>::prototype(context)?
        .expect("macro-backed Node class has a prototype");
    node_methods.prop(
        WRAPPER_CACHE,
        Class::instance(
            context.clone(),
            WrapperCache {
                entries: Vec::new(),
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

fn cached_wrapper<'js>(
    cache: &Class<'js, WrapperCache<'js>>,
    id: NodeId,
) -> Result<Option<Object<'js>>> {
    let Some(reference) = cache.borrow().reference(id) else {
        return Ok(None);
    };
    let deref: Function = reference.get("deref")?;
    deref.call((rquickjs::function::This(reference),))
}

fn cache_wrapper<'js>(
    context: &Ctx<'js>,
    cache: &Class<'js, WrapperCache<'js>>,
    id: NodeId,
    wrapper: &Object<'js>,
) -> Result<()> {
    let weak_ref: Constructor = context.globals().get("WeakRef")?;
    cache.borrow_mut().entries.push(WrapperEntry {
        id,
        reference: weak_ref.construct((wrapper.clone(),))?,
    });
    Ok(())
}

fn wrap_node<'js>(context: &Ctx<'js>, state: &SharedUiDom, id: NodeId) -> Result<Object<'js>> {
    let token = encode_node_id(id);
    if let Some(cached) = cached_wrapper(context, &token)? {
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
        context,
        token,
        node.as_value()
            .as_object()
            .expect("class instance is an object"),
    )?;
    Ok(node.into_inner())
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
