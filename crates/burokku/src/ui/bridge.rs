use runtime::rquickjs::{Ctx, Function, Result};

use super::{Elements, UiStore};

pub fn install<'js>(context: &Ctx<'js>, store: UiStore) -> Result<()> {
    let render = Function::new(context.clone(), move |root: Elements| -> Result<()> {
        store.replace(root);
        Ok(())
    })?;
    context.globals().set("__burokku_render", render)
}
