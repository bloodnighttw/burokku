#[cfg(target_os = "macos")]
mod macos {
    use std::{
        cell::Cell,
        process::{Command, Stdio},
        rc::Rc,
        thread,
        time::{Duration, Instant},
    };

    use burokku_winit::{application::ApplicationHandler, ActiveEventLoop, EventLoop};
    use tokio::{sync::oneshot, task::LocalSet};

    const CHILD: &str = "BUROKKU_EXTERNAL_WAKE_TEST_CHILD";

    struct App {
        completed: Rc<Cell<bool>>,
        exited: bool,
    }

    impl ApplicationHandler for App {
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.completed.get() {
                event_loop.exit();
            }
        }

        fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
            self.exited = true;
        }
    }

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
        let completed = Rc::new(Cell::new(false));
        let (sender, receiver) = oneshot::channel();

        local_set.spawn_local({
            let completed = Rc::clone(&completed);
            async move {
                receiver.await.unwrap();
                completed.set(true);
            }
        });
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            sender.send(()).unwrap();
        });

        let app = event_loop
            .run_app_external(
                App {
                    completed,
                    exited: false,
                },
                local_set,
            )
            .unwrap();
        assert!(app.completed.get());
        assert!(app.exited);
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
}
