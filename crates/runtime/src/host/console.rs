use crate::Result;
use rquickjs::{prelude::Func, Ctx, Object};

pub(crate) fn install<'js>(context: &Ctx<'js>) -> Result<()> {
    let console = Object::new(context.clone())?;
    console.set(
        "log",
        Func::from(|message: String| {
            println!("{message}");
        }),
    )?;
    context.globals().set("console", console)?;
    Ok(())
}
