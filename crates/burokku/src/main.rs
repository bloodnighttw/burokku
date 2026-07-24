#![forbid(unsafe_code)]

use std::error::Error;

use runtime::{Runtime, WindowEventMessage};
use tokio::sync::mpsc::{self, UnboundedReceiver};

mod ui;
mod window;

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
    borderStyle: "solid",
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
    let ui_store = ui::UiStore::new();
    if check_only {
        return check_ui(ui_store, source).await;
    }
    let (window_events_tx, window_events_rx) = mpsc::unbounded_channel();
    let js_ui = ui_store.clone();
    let js_task = tokio::spawn(run_javascript(window_events_rx, js_ui, source));

    let window_result = window::run(window_events_tx, ui_store).await;
    let js_result = js_task.await?;

    window_result?;
    js_result.map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}

async fn check_ui(store: ui::UiStore, source: String) -> Result<(), Box<dyn Error>> {
    let host_store = store.clone();
    let runtime = Runtime::new_with_host(move |context| ui::install(context, host_store)).await?;
    runtime.eval::<()>(source).await?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let snapshot = store.snapshot();
    if snapshot.body().children.is_empty() {
        return Err("script did not attach any nodes to document.body".into());
    }
    let mut text_system = render::TextSystem::new();
    let canvas = ui::build_canvas(&snapshot, 800.0, 600.0, 1.0, &mut text_system);
    println!(
        "UI check: {} root nodes, {} drawing commands",
        snapshot.body().children.len(),
        canvas.commands().len()
    );
    Ok(())
}

async fn run_javascript(
    mut window_events_rx: UnboundedReceiver<WindowEventMessage>,
    store: ui::UiStore,
    source: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let runtime = Runtime::new_with_host(move |context| ui::install(context, store)).await?;
    runtime.eval::<()>(source).await?;

    let mut batch = Vec::with_capacity(16);

    while let Some(event) = window_events_rx.recv().await {
        batch.push(event);
        while let Ok(event) = window_events_rx.try_recv() {
            batch.push(event);
        }

        runtime.enqueue_window_events(&batch).await?;
        batch.clear();
    }

    Ok(())
}
