//! Platform-owned main loop driving Tokio and LLRT through burokku-winit.
//!
//! The example contains no native platform APIs. `EventLoop::run_app_external`
//! selects the backend that supplies the native wake source and deadline timer.
//! macOS is the only implemented backend today.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use burokku_winit::{
    application::ApplicationHandler, ActiveEventLoop, EventLoop, LogicalSize, Window, WindowEvent,
    WindowId,
};
use llrt_utils::primordials::{BasePrimordials, Primordial};
use tokio::task::LocalSet;

const LLRT_SCRIPT: &str = include_str!("cf_run_loop_tokio.js");

fn install_llrt_globals(context: &runtime::rquickjs::Ctx<'_>) -> runtime::Result<()> {
    BasePrimordials::init(context)?;
    let (_, _, globals) = llrt_modules::module_builder::ModuleBuilder::default().build();
    globals.attach(context)
}

struct App {
    window: Option<Window>,
    failure: Rc<RefCell<Option<String>>>,
}

impl App {
    fn new(failure: Rc<RefCell<Option<String>>>) -> Self {
        Self {
            window: None,
            failure,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title("LLRT + Tokio external loop — live-resize for at least five seconds")
            .with_inner_size(LogicalSize::new(720.0, 420.0));
        self.window = Some(
            event_loop
                .create_window(attributes)
                .expect("failed to create the example window"),
        );
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.failure.borrow().is_some() {
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.id() != window_id {
            return;
        }

        if event == WindowEvent::CloseRequested {
            window.close();
            event_loop.exit();
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop = EventLoop::new()?;
    let failure_proxy = event_loop.create_proxy();
    let main_thread = std::thread::current().id();
    let local_set = LocalSet::new();
    let failure = Rc::new(RefCell::new(None));

    local_set.spawn_local({
        let failure = Rc::clone(&failure);
        async move {
            assert_eq!(std::thread::current().id(), main_thread);
            let (javascript, driver) = match runtime::Runtime::builder()
                .plugin(install_llrt_globals)
                .build_driven()
                .await
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    *failure.borrow_mut() = Some(format!(
                        "failed to initialize the LLRT-backed runtime: {error}"
                    ));
                    failure_proxy.wake_up();
                    return;
                }
            };
            let driver = tokio::task::spawn_local(driver.run());

            if let Err(error) = javascript.eval::<()>(LLRT_SCRIPT).await {
                *failure.borrow_mut() =
                    Some(format!("failed to evaluate the LLRT test script: {error}"));
                failure_proxy.wake_up();
                return;
            }
            assert_eq!(std::thread::current().id(), main_thread);

            let message = match driver.await {
                Ok(()) => "LLRT JavaScript driver stopped unexpectedly".to_owned(),
                Err(error) => format!("LLRT JavaScript driver failed: {error}"),
            };
            *failure.borrow_mut() = Some(message);
            failure_proxy.wake_up();
        }
    });

    // Opt in to the expected limitation:
    // TOKIO_EXTERNAL_CPU_BLOCK=1 cargo run -p burokku-winit --example cf_run_loop_tokio
    if std::env::var_os("TOKIO_EXTERNAL_CPU_BLOCK").is_some() {
        local_set.spawn_local(async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            println!("starting expected two-second main-thread blockage");
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                std::hint::spin_loop();
            }
            println!("main-thread blockage ended");
        });
    }

    let app = event_loop.run_app_external(App::new(Rc::clone(&failure)), local_set)?;
    if let Some(error) = app.failure.borrow_mut().take() {
        return Err(std::io::Error::other(error).into());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cf_run_loop_tokio failed: {error}");
    }
}
