#![forbid(unsafe_code)]

use std::error::Error;

use runtime::{Runtime, WindowEventMessage};
use tokio::sync::mpsc::{self, Receiver};

mod dom;
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
    let dom = dom::DomStore::new();
    if check_only {
        return check_dom(dom, source).await;
    }
    let (window_events_tx, window_events_rx) = mpsc::channel(256);
    let js_dom = dom.clone();
    let js_task = tokio::spawn(run_javascript(window_events_rx, js_dom, source));

    let window_result = window::run(window_events_tx, dom).await;
    let js_result = js_task.await?;

    window_result?;
    js_result.map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}

async fn check_dom(dom: dom::DomStore, source: String) -> Result<(), Box<dyn Error>> {
    let host_dom = dom.clone();
    let runtime = Runtime::new_with_host(move |context| dom::install(context, host_dom)).await?;
    runtime.eval::<()>(source).await?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let snapshot = dom.snapshot();
    if snapshot.body().children.is_empty() {
        return Err("script did not attach any nodes to document.body".into());
    }
    let mut text_system = render::TextSystem::new();
    let canvas = dom::build_canvas(&snapshot, 800.0, 600.0, 1.0, &mut text_system)?;
    println!(
        "DOM check: {} root nodes, {} drawing commands",
        snapshot.body().children.len(),
        canvas.commands().len()
    );
    Ok(())
}

async fn run_javascript(
    mut window_events_rx: Receiver<WindowEventMessage>,
    dom: dom::DomStore,
    source: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let runtime = Runtime::new_with_host(move |context| dom::install(context, dom)).await?;
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
