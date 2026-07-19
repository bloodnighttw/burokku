use runtime::Runtime;
use std::{ffi::OsString, path::PathBuf, time::Duration};
use tokio::{sync::mpsc, time::timeout};

mod ui;
mod window;

const DEFAULT_SCRIPT: &str = r#"
(async () => {
    console.log("A");

    setTimeout(() => console.log("B"), 0);

    Promise.resolve().then(() => console.log("C"));

    console.log("D");

    // Keep the top-level promise alive long enough for the timer macrotask.
    await new Promise(resolve => setTimeout(resolve, 0));
})();
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run().await
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mode = RunMode::from_args(std::env::args_os().skip(1))?;
    let Some((script_path, open_window)) = mode.ui_script() else {
        let runtime = Runtime::new().await?;
        runtime.eval_promise::<()>(DEFAULT_SCRIPT).await?;
        return Ok(());
    };
    let source = tokio::fs::read_to_string(script_path).await?;

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let runtime =
        Runtime::new_with_host(move |context| ui::bridge::install(context, sender)).await?;
    runtime.eval::<()>(source).await?;
    let mut document = ui::UiDocument::new();
    let mutation_count = receive_flush(
        &mut document,
        &mut receiver,
        Duration::from_secs(1),
        "UI script finished without committing a React tree",
    )
    .await?;
    println!(
        "[Burokku perf] Host commit #{}: applied {} native mutations",
        document.commit_id, mutation_count
    );
    if open_window {
        window::run(document, receiver)?;
    } else {
        let mut text_system = render::TextSystem::new();
        let layout = ui::UiLayout::compute(&document, 800.0, 600.0, &mut text_system)?;
        let canvas = layout.paint(render::Color::WHITE)?;
        let size = layout.root_size()?;
        println!(
            "UI layout: {:.0}x{:.0}, {} drawing commands",
            size.width,
            size.height,
            canvas.commands().len()
        );
        let initial_commit_id = document.commit_id;
        receive_flush(
            &mut document,
            &mut receiver,
            Duration::from_millis(1_500),
            "UI runtime stopped before producing a state update",
        )
        .await?;
        if document.commit_id == initial_commit_id {
            return Err("UI state update did not change the rendered tree".into());
        }
        let updated_layout = ui::UiLayout::compute(&document, 800.0, 600.0, &mut text_system)?;
        let updated_canvas = updated_layout.paint(render::Color::WHITE)?;
        println!(
            "UI state update: {} drawing commands",
            updated_canvas.commands().len()
        );
    }

    Ok(())
}

async fn receive_flush(
    document: &mut ui::UiDocument,
    receiver: &mut mpsc::UnboundedReceiver<ui::UiUpdate>,
    duration: Duration,
    closed_message: &'static str,
) -> Result<usize, Box<dyn std::error::Error>> {
    timeout(duration, async {
        let mut mutation_count = 0;
        loop {
            let update = receiver
                .recv()
                .await
                .ok_or_else(|| std::io::Error::other(closed_message))?;
            if matches!(update, ui::UiUpdate::Mutation(_)) {
                mutation_count += 1;
            }
            if document.apply(update)? {
                return Ok::<_, Box<dyn std::error::Error>>(mutation_count);
            }
        }
    })
    .await?
}

enum RunMode {
    Default,
    Window(PathBuf),
    Check(PathBuf),
}

impl RunMode {
    fn from_args(mut args: impl Iterator<Item = OsString>) -> Result<Self, &'static str> {
        let Some(first) = args.next() else {
            return Ok(Self::Default);
        };
        if first == "--check-ui" {
            let path = args.next().ok_or("--check-ui requires a JavaScript file")?;
            return Ok(Self::Check(path.into()));
        }
        Ok(Self::Window(first.into()))
    }

    fn ui_script(&self) -> Option<(&PathBuf, bool)> {
        match self {
            Self::Default => None,
            Self::Window(path) => Some((path, true)),
            Self::Check(path) => Some((path, false)),
        }
    }
}
