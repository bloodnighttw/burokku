use std::{cell::Ref, rc::Rc};

use rquickjs::{
    class::Trace, object::Property, prelude::Func, Class, Coerced, Ctx, Function, IntoJs,
    JsLifetime, Null, Object, Result, Value,
};

use super::{
    errors,
    lifetime::{decode_node_id, encode_node_id},
    LayoutRect, SharedUiDom, UiDomState,
};
use crate::ui::elements::{DomError, ElementTag, NodeId, NodeKind};

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "NativeNode", frozen)]
pub(super) struct NativeNode {
    #[qjs(skip_trace)]
    state: SharedUiDom,
    #[qjs(skip_trace)]
    id: NodeId,
}

#[rquickjs::methods]
impl<'js> NativeNode {
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

    #[qjs(rename = "appendChild")]
    fn append_child(
        &self,
        context: Ctx<'js>,
        child: Class<'js, NativeNode>,
    ) -> Result<Class<'js, NativeNode>> {
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
        child: Class<'js, NativeNode>,
        reference: Option<Class<'js, NativeNode>>,
    ) -> Result<Class<'js, NativeNode>> {
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
        child: Class<'js, NativeNode>,
    ) -> Result<Class<'js, NativeNode>> {
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
        new_child: Class<'js, NativeNode>,
        old_child: Class<'js, NativeNode>,
    ) -> Result<Class<'js, NativeNode>> {
        let new_id = self.related_id(&context, &new_child)?;
        let old_id = self.related_id(&context, &old_child)?;
        let result = borrow_mut(&context, &self.state)?
            .dom
            .replace_child(self.id, new_id, old_id);
        errors::map_dom(&context, "replaceChild", result)?;
        Ok(old_child)
    }

    fn contains(&self, context: Ctx<'js>, other: Class<'js, NativeNode>) -> Result<bool> {
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
    fn related_id(&self, context: &Ctx<'js>, node: &Class<'js, NativeNode>) -> Result<NodeId> {
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

pub(super) fn install(context: &Ctx<'_>, state: SharedUiDom) -> Result<()> {
    let node_methods =
        Class::<NativeNode>::prototype(context)?.expect("macro-backed Node class has a prototype");
    let style_methods = Class::<NativeStyleDeclaration>::prototype(context)?
        .expect("macro-backed style class has a prototype");
    let release_wrapper = Func::from({
        let state = state.clone();
        move |context: Ctx<'_>, token: String| -> Result<()> {
            let id = match decode_node_id(&token) {
                Ok(id) => id,
                Err(_) => return errors::invalid_token(&context),
            };
            borrow_mut(&context, &state)?.release_wrapper(id);
            Ok(())
        }
    });
    let bootstrap: Function = context.eval(include_str!("../scripts/dom_facade.js"))?;
    bootstrap.call::<_, ()>((node_methods, style_methods, release_wrapper))?;

    let root = borrow(context, &state)?.dom.root();
    let app = wrap_node(context, &state, root)?;
    context
        .globals()
        .prop("app", Property::from(app).enumerable())?;
    Ok(())
}

fn wrap_node<'js>(context: &Ctx<'js>, state: &SharedUiDom, id: NodeId) -> Result<Object<'js>> {
    let token = encode_node_id(id);
    let methods =
        Class::<NativeNode>::prototype(context)?.expect("macro-backed Node has a prototype");
    let get_cached: Function = methods.get("getCachedWrapper")?;
    if let Some(cached) = get_cached.call::<_, Option<Object>>((token.clone(),))? {
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
    {
        let mut state = borrow_mut(context, state)?;
        errors::map_dom(context, "acquire node wrapper", state.acquire_wrapper(id))?;
    }

    let result = (|| {
        let constructor: Object = context.globals().get(constructor_name)?;
        let prototype: Object = constructor.get("prototype")?;
        let node = Class::instance_proto(
            NativeNode {
                state: state.clone(),
                id,
            },
            prototype,
        )?;

        if is_element {
            let constructor: Object = context.globals().get("BurokkuStyleDeclaration")?;
            let prototype: Object = constructor.get("prototype")?;
            let style = Class::instance_proto(
                NativeStyleDeclaration {
                    state: state.clone(),
                    id,
                },
                prototype,
            )?;
            node.prop("style", Property::from(style))?;
        }

        let cache: Function = methods.get("cacheWrapper")?;
        cache.call::<_, ()>((token, node.clone()))?;
        Ok(node.into_inner())
    })();

    if result.is_err() {
        if let Ok(mut state) = state.try_borrow_mut() {
            state.release_wrapper(id);
        }
    }
    result
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
