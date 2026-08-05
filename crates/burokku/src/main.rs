#![forbid(unsafe_code)]

use std::error::Error;

use runtime::{Runtime, WindowEventMessage};
use tokio::sync::mpsc::{self, UnboundedReceiver};

mod ui;
mod window;

const DEFAULT_SCRIPT: &str = r##"
__burokku_render(JSON.stringify({
  type: "app",
  children: [{
    type: "window",
    children: [{
      type: "flex",
      style: {
        flexDirection: "column",
        gap: 12,
        backgroundColor: "#f5f7fa",
        borderColor: "#cbd2dc",
        borderWidth: 1,
        borderRadius: 16
      },
      children: [{
        type: "text",
        style: { color: "#18202b", fontSize: 28, lineHeight: 34, fontWeight: 700 },
        children: [{ type: "string", value: "Burokku UI" }]
      }, {
        type: "text",
        style: { color: "#526071", fontSize: 16, lineHeight: 24 },
        children: [{ type: "string", value: "React and Solid render this native element tree." }]
      }]
    }]
  }]
}));
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

    let snapshot = ui::Elements::from_json(&store.snapshot())?;
    let ui::Elements::App { children } = &snapshot else {
        return Err("script did not render an app root".into());
    };
    if !children
        .iter()
        .any(|child| matches!(child, ui::Elements::Window { .. }))
    {
        return Err("script did not render a window".into());
    }
    let mut text_system = render::TextSystem::new();
    let canvas = ui::build_canvas(&snapshot, 800.0, 600.0, 1.0, &mut text_system);
    println!(
        "UI check: {} windows, {} drawing commands",
        children.len(),
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
