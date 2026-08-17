use std::{error::Error, path::PathBuf};

use burokku::{
    runtime::{
        plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
        DualRuntime,
    },
    Burokku,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dist/app.js");
    let source = tokio::fs::read(&bundle).await.map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "failed to read {} ({error}); build it with `pnpm build` first",
                bundle.display()
            ),
        )
    })?;

    let app = Burokku::builder()
        .title("Burokku Counter")
        .dual_runtime(DualRuntime::builder())
        .main_runtime_plugin(ConsolePlugin)
        .runtime_plugin(ConsolePlugin)
        .runtime_plugin(JsonPlugin)
        .runtime_plugin(TimersPlugin)
        .background_script(source);

    if std::env::args_os().any(|argument| argument == "--check") {
        app.headless().run().await?;
    } else {
        app.run().await?;
    }
    Ok(())
}
