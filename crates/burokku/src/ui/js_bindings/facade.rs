//! JavaScript DOM constructor and prototype facade.

use rquickjs::{Constructor, Ctx, Function, Object, Result};

pub(super) fn install<'js>(
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
            "setPointerCapture",
            "releasePointerCapture",
            "hasPointerCapture",
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
