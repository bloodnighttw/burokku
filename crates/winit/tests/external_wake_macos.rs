#[cfg(target_os = "macos")]
mod macos {
    use std::{
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use burokku_winit::{application::ApplicationHandler, EventLoop};
    use tokio::{sync::oneshot, task::LocalSet};

    const CHILD: &str = "BUROKKU_EXTERNAL_WAKE_TEST_CHILD";

    struct App;
    impl ApplicationHandler for App {}

    pub fn run() {
        if std::env::var_os(CHILD).is_some() {
            child();
            return;
        }

        let mut child = Command::new(std::env::current_exe().unwrap())
            .env(CHILD, "1")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "external wake child failed: {status}");
                return;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("worker-side oneshot did not wake the macOS event loop");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn child() {
        let mut event_loop = EventLoop::new().unwrap();
        let local_set = LocalSet::new();
        let (sender, receiver) = oneshot::channel();

        local_set.spawn_local(async move {
            receiver.await.unwrap();
            std::process::exit(0);
        });
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            sender.send(()).unwrap();
        });

        event_loop.run_app_external(App, local_set).unwrap();
        panic!("native event loop exited before the worker-side oneshot completed");
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
}
