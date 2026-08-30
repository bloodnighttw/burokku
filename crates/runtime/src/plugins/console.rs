use crate::{Plugin, Result};
use rquickjs::{prelude::Func, Ctx, Object};

/// Installs the standard `console` global.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn install<'js>(&self, context: &Ctx<'js>) -> Result<()> {
        let console = Object::new(context.clone())?;
        let log = Func::from(|message: String| {
            println!("{message}");
        });
        console.set("log", log)?;
        context.globals().set("console", console)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ConsolePlugin;
    use crate::Runtime;
    use tokio::task::LocalSet;

    #[tokio::test(flavor = "current_thread")]
    async fn exposes_a_javascript_console_log_function() {
        LocalSet::new()
            .run_until(async {
                let (runtime, driver) = Runtime::builder()
                    .plugin(ConsolePlugin)
                    .build_driven()
                    .await
                    .unwrap();
                let driver = tokio::task::spawn_local(driver.run());
                let is_console_log: bool = runtime
                    .eval(include_str!("scripts/console.js"))
                    .await
                    .unwrap();
                assert!(is_console_log);
                runtime.shutdown().await.unwrap();
                driver.await.unwrap();
            })
            .await;
    }
}
