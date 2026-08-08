#![forbid(unsafe_code)]

use std::error::Error;

use runtime::{
    plugins::{ConsolePlugin, JsonPlugin, TimersPlugin, WindowEventsPlugin},
    Runtime,
};

mod ui;

const DEFAULT_SCRIPT: &str = r##"
const card = document.createElement("div");
Object.assign(card.style, {
    display: "flex",
    flexDirection: "column",
    width: "360px",
    margin: "32px",
    padding: "24px",
    gap: "12px",
    backgroundColor: "#f5f7fa",
    borderColor: "#cbd2dc",
    borderWidth: "1px",
    borderRadius: "16px",
});

const title = document.createElement("span");
Object.assign(title.style, { color: "#18202b", fontSize: "28px", lineHeight: "34px", fontWeight: "700" });
title.textContent = "Burokku DOM";

const detail = document.createElement("span");
Object.assign(detail.style, { color: "#526071", fontSize: "16px", lineHeight: "24px" });
detail.textContent = "Solid and React can mutate this native tree with familiar DOM operations.";

card.append(title, detail);
document.body.appendChild(card);
"##;

#[tokio::main(flavor = "multi_thread")]
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
        None => DEFAULT_SCRIPT.into(),
    };
    let js_task = tokio::spawn(run_javascript(source));

    let js_result = js_task.await?;

    js_result.map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}

async fn run_javascript(source: String) -> Result<(), Box<dyn Error + Send + Sync>> {
    let runtime = Runtime::builder()
        .plugin(ConsolePlugin)
        .plugin(JsonPlugin)
        .plugin(TimersPlugin)
        .plugin(WindowEventsPlugin::default())
        .build()
        .await?;
    runtime.eval::<()>(source).await?;

    Ok(())
}
