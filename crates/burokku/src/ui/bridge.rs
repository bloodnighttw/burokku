use runtime::rquickjs::{Ctx, Error, Function, Result};

use super::{ElementKind, UiStore};

pub fn install<'js>(context: &Ctx<'js>, store: UiStore) -> Result<()> {
    let create_store = store.clone();
    context.globals().set(
        "__burokku_dom_create",
        Function::new(
            context.clone(),
            move |kind: String, name: String| -> Result<u64> {
                let kind = match kind.as_str() {
                    "element" => ElementKind::from(name),
                    "text" => ElementKind::Text(String::new()),
                    "comment" => ElementKind::Comment(String::new()),
                    _ => return Err(js_error(format!("unsupported DOM node kind '{kind}'"))),
                };
                Ok(create_store.create_node(kind))
            },
        )?,
    )?;

    let insert_store = store.clone();
    context.globals().set(
        "__burokku_dom_insert",
        Function::new(
            context.clone(),
            move |parent: u64, child: u64, before: i64| -> Result<()> {
                insert_store
                    .insert(parent, child, u64::try_from(before).ok())
                    .map_err(|error| js_error(error.to_string()))
            },
        )?,
    )?;

    let remove_store = store.clone();
    context.globals().set(
        "__burokku_dom_remove",
        Function::new(
            context.clone(),
            move |parent: u64, child: u64| -> Result<()> {
                remove_store
                    .remove(parent, child)
                    .map_err(|error| js_error(error.to_string()))
            },
        )?,
    )?;

    let text_store = store.clone();
    context.globals().set(
        "__burokku_dom_set_text",
        Function::new(
            context.clone(),
            move |id: u64, text: String| -> Result<()> {
                text_store
                    .set_text(id, text)
                    .map_err(|error| js_error(error.to_string()))
            },
        )?,
    )?;

    let style_store = store.clone();
    context.globals().set(
        "__burokku_dom_set_style",
        Function::new(
            context.clone(),
            move |id: u64, name: String, value: Option<String>| -> Result<()> {
                style_store
                    .set_style(id, &name, value.as_deref())
                    .map_err(|error| js_error(error.to_string()))
            },
        )?,
    )?;

    let attribute_store = store;
    context.globals().set(
        "__burokku_dom_set_attribute",
        Function::new(
            context.clone(),
            move |id: u64, name: String, value: Option<String>| -> Result<()> {
                attribute_store
                    .set_attribute(id, &name, value.as_deref())
                    .map_err(|error| js_error(error.to_string()))
            },
        )?,
    )?;

    context.eval::<(), _>(include_str!("bootstrap.js"))?;
    Ok(())
}

fn js_error(message: String) -> Error {
    Error::new_from_js_message("DOM operation", "native UI", message)
}
