#![forbid(unsafe_code)]

use std::error::Error;

use runtime::{Runtime, WindowEventMessage};
use tokio::sync::mpsc::{self, Receiver};

mod window;

const DEFAULT_SCRIPT: &str = r#"
let resizeEvents = 0;
let latestResize = null;

globalThis.__burokku_dispatch_event = event => {
    if (event.type === "resized") {
        resizeEvents += 1;
        latestResize = event;
        console.log(`[JS] Event`);

        if (resizeEvents === 1) {
            console.log(`[JS] first resize: ${event.width}x${event.height}`);
        }
    } else {
        console.log(`[JS] received Winit event: ${event.type}`);
    }
};

console.log("[JS] event loop started");

setInterval(() => {
    const size = latestResize
        ? `, latest size: ${latestResize.width}x${latestResize.height}`
        : "";
    console.log(`[JS] heartbeat; resize events received: ${resizeEvents}${size}`);
}, 1000);
"#;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let (window_events_tx, window_events_rx) = mpsc::channel(256);
    let js_task = tokio::spawn(run_javascript(window_events_rx));

    let window_result = window::run(window_events_tx).await;
    let js_result = js_task.await?;

    window_result?;
    js_result.map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}

async fn run_javascript(
    mut window_events_rx: Receiver<WindowEventMessage>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let runtime = Runtime::new().await?;
    runtime.eval::<()>(DEFAULT_SCRIPT).await?;

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
