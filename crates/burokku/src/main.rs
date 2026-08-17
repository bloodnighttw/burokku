#![forbid(unsafe_code)]

use std::{
    error::Error,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use runtime::{
    plugins::{ConsolePlugin, JsonPlugin, TimersPlugin},
    DualRuntime,
};
use tokio::sync::oneshot;
use winit::{EventLoop, Window};

use burokku::ui::{
    self,
    events::EventDispatcher,
    frame::{FrameRenderer, UiApplication},
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
        return Err("--check-dom requires a JavaScript file".into());
    }
    let source = match script_path {
        Some(path) => tokio::fs::read_to_string(path).await?,
        None => BACKGROUND_SCRIPT.into(),
    };

    if check_only {
        run_headless(source).await?;
    } else {
        run_windowed(source).await?;
    }
    Ok(())
}

/// Evaluate a DOM script without creating native or GPU state.
async fn run_headless(source: String) -> runtime::Result<()> {
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

/// Run the native event loop, MTS JavaScript driver, and BTS application
/// concurrently on their assigned threads.
async fn run_windowed(source: String) -> Result<(), Box<dyn Error>> {
    // Native window and GPU resources are created on MTS before either
    // JavaScript isolate starts producing DOM commits.
    let mut event_loop = EventLoop::new()?;
    let window =
        Arc::new(event_loop.create_window(Window::default_attributes().with_title("Burokku"))?);
    let renderer = FrameRenderer::new(window.clone()).await?;

    // BTS commits can happen while the demand-driven native loop is waiting.
    // Wake it at publication time instead of waiting for the fallback poll.
    let event_loop_proxy = event_loop.create_proxy();
    let shared_dom = ui::elements::SharedDom::with_commit_waker(move || {
        event_loop_proxy.wake_up();
    });
    let dom_plugin = ui::js_bridge::DomPlugin::new(shared_dom.clone());
    let (runtime, main_driver) = DualRuntime::builder()
        .main_plugin(ConsolePlugin)
        .background_plugin(ConsolePlugin)
        .background_plugin(JsonPlugin)
        .background_plugin(TimersPlugin)
        .background_plugin(dom_plugin)
        .build()
        .await?;

    let event_dispatcher = EventDispatcher::new(runtime.background().macrotask_queue());
    let (close_sender, close_receiver) = oneshot::channel();
    let external_exit = Arc::new(AtomicBool::new(false));
    let application = UiApplication::new(
        window,
        shared_dom,
        renderer,
        event_dispatcher,
        close_sender,
        external_exit.clone(),
    );

    let runtime_lifecycle = async move {
        let (main_result, background_result) = tokio::join!(
            runtime.main().eval::<()>(MAIN_SCRIPT),
            runtime.background().eval::<()>(source),
        );
        let evaluation_result = match (main_result, background_result) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        };

        if evaluation_result.is_err() {
            external_exit.store(true, Ordering::Release);
        } else {
            // Keep both runtimes alive until the final native window closes or
            // the renderer asks the event loop to exit.
            let _ = close_receiver.await;
        }

        let shutdown_result = runtime.shutdown().await;
        evaluation_result?;
        shutdown_result
    };

    // All three futures are cooperatively polled by the current-thread Tokio
    // runtime. QuickJS MTS work and native callbacks therefore remain on the
    // process main thread, while DualRuntime owns and joins the BTS thread.
    let ((), native_result, runtime_result) = tokio::join!(
        main_driver.run(),
        event_loop.run_app(application),
        runtime_lifecycle,
    );

    let mut application = native_result?;
    runtime_result?;
    if std::env::var_os("BUROKKU_PRINT_METRICS").is_some() {
        eprintln!("Burokku performance metrics: {:#?}", application.metrics());
    }
    if let Some(error) = application.take_error() {
        return Err(Box::new(error));
    }
    Ok(())
}
