use std::{error::Error, thread};

use runtime::{Runtime, WindowEventMessage};
use tokio::{
    runtime::Builder,
    sync::mpsc::{self, Receiver},
};

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

fn main() -> Result<(), Box<dyn Error>> {
    let (window_events_tx, window_events_rx) = mpsc::channel(256);
    let js_thread = thread::Builder::new()
        .name("burokku-js".into())
        .spawn(move || run_javascript(window_events_rx))?;

    let window_result = window::run(window_events_tx);
    let js_result = js_thread
        .join()
        .map_err(|_| std::io::Error::other("JavaScript thread panicked"))?;

    window_result?;
    js_result.map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}

fn run_javascript(
    mut window_events_rx: Receiver<WindowEventMessage>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let tokio = Builder::new_current_thread().enable_all().build()?;

    Ok(tokio.block_on(async move {
        let runtime = Runtime::new().await?;
        runtime.eval::<()>(DEFAULT_SCRIPT).await?;

        let mut batch = Vec::with_capacity(16);

        while let Some(event) = window_events_rx.recv().await {
            batch.push(event);
            while let Ok(event) = window_events_rx.try_recv() {
                batch.push(event);
            }

            let mut latest_resize = None;
            let mut close_requested = false;
            for event in batch.drain(..) {
                match event {
                    WindowEventMessage::CloseRequested => close_requested = true,
                    resize @ WindowEventMessage::Resized { .. } => latest_resize = Some(resize),
                }
            }
            if let Some(resize) = latest_resize {
                batch.push(resize);
            }
            if close_requested {
                batch.push(WindowEventMessage::CloseRequested);
            }

            runtime.enqueue_window_events(&batch).await?;
            batch.clear();
        }

        Ok::<(), runtime::Error>(())
    })?)
}
