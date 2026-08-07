use crate::{Plugin, Result};
use rquickjs::{prelude::Func, Ctx, Object};

/// Installs the standard `console` global.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
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
}
