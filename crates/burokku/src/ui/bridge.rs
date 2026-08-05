use runtime::rquickjs::{Ctx, Error, Function, Result};

use super::{Elements, UiStore};

pub fn install<'js>(context: &Ctx<'js>, store: UiStore) -> Result<()> {
    let render = Function::new(context.clone(), move |serialized: String| -> Result<()> {
        let root = Elements::from_json(&serialized).map_err(|error| {
            Error::new_from_js_message("element tree", "native UI", error.to_string())
        })?;
        drop(root);
        store.replace(serialized);
        Ok(())
    })?;
    context.globals().set("__burokku_render", render)
}
