#![forbid(unsafe_code)]

use std::error::Error;

use burokku::{
    runtime::{
        plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
        DualRuntime,
    },
    Burokku,
};

const MAIN_SCRIPT: &str = r#"console.log("Hello, world from the main runtime!");"#;
const BACKGROUND_SCRIPT: &str = r#"console.log("Hello, world from the background runtime!");"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    let (check_only, script_path) = match first.as_deref() {
        Some(flag) if flag == "--check-dom" => (true, args.next()),
        path => (false, path.map(Into::into)),
    };
    if check_only && script_path.is_none() {
        return Err("--check-dom requires a bundled JavaScript file".into());
    }

    let source = match script_path {
        Some(path) => tokio::fs::read(path).await?,
        None => BACKGROUND_SCRIPT.as_bytes().to_vec(),
    };

    let builder = Burokku::builder()
        .dual_runtime(DualRuntime::builder())
        .main_runtime_plugin(ConsolePlugin)
        .runtime_plugin(ConsolePlugin)
        .runtime_plugin(JsonPlugin)
        .runtime_plugin(TimersPlugin)
        .main_script(MAIN_SCRIPT)
        .background_script(source);

    if check_only {
        builder.headless().run().await?;
    } else {
        builder.run().await?;
    }
    Ok(())
}
