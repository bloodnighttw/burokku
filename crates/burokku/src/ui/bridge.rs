use runtime::rquickjs::{Ctx, Function, Result};
use tokio::sync::mpsc::UnboundedSender;

pub fn install<'js>(context: &Ctx<'js>, sender: UnboundedSender<String>) -> Result<()> {
    let commit = Function::new(context.clone(), move |snapshot: String| {
        let _ = sender.send(snapshot);
    })?;
    context.globals().set("__burokku_commit", commit)?;
    Ok(())
}
