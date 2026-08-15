#![forbid(unsafe_code)]

use std::error::Error;

use runtime::{
    plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
    DualRuntime,
};

mod ui;

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
        return Err("--check-dom requires a JavaScript file".into());
    }
    let source = match script_path {
        Some(path) => tokio::fs::read_to_string(path).await?,
        None => BACKGROUND_SCRIPT.into(),
    };
    run_javascript(source).await?;
    Ok(())
}

async fn run_javascript(source: String) -> runtime::Result<()> {
    let (dom_plugin, _shared_dom) = ui::js_bridge::DomPlugin::with_new_dom();
    let (runtime, main_driver) = DualRuntime::builder()
        .main_plugin(ConsolePlugin)
        .background_plugin(ConsolePlugin)
        .background_plugin(JsonPlugin)
        .background_plugin(TimersPlugin)
        .background_plugin(dom_plugin)
        .build()
        .await?;

    let application = async move {
        let (main_result, background_result) = tokio::join!(
            runtime.main().eval::<()>(MAIN_SCRIPT),
            runtime.background().eval::<()>(source),
        );
        let shutdown_result = runtime.shutdown().await;

        main_result?;
        background_result?;
        shutdown_result
    };

    let ((), result) = tokio::join!(main_driver.run(), application);
    result
}
