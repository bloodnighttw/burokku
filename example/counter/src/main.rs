use std::error::Error;

use burokku::{
    runtime::{
        plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
        DualRuntime,
    },
    Burokku,
};

const APP_SCRIPT: &str = include_str!("../dist/app.js");

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let app = Burokku::builder()
        .title("Burokku Counter")
        .dual_runtime(DualRuntime::builder())
        .main_runtime_plugin(ConsolePlugin)
        .runtime_plugin(ConsolePlugin)
        .runtime_plugin(JsonPlugin)
        .runtime_plugin(TimersPlugin)
        .background_script(APP_SCRIPT);

    if std::env::args_os().any(|argument| argument == "--check") {
        app.headless().run().await?;
    } else {
        app.run().await?;
    }
    Ok(())
}
