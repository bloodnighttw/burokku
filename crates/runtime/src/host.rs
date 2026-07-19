pub(crate) mod console;
pub(crate) mod timers;

use crate::Result;
use rquickjs::Ctx;

pub(crate) fn install<'js>(context: &Ctx<'js>) -> Result<()> {
    console::install(context)?;
    timers::install(context)?;
    Ok(())
}
