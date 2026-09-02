#[cfg(target_os = "macos")]
mod macos {
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
        process::{Command, Stdio},
        rc::Rc,
        thread,
        time::{Duration, Instant},
    };

    use burokku_winit::{application::ApplicationHandler, ActiveEventLoop, EventLoop};
    use tokio::{sync::oneshot, task::LocalSet};

    const CHILD: &str = "BUROKKU_EXTERNAL_WAKE_TEST_CHILD";
    const WAKE_CHILD: &str = "wake";
    const PANIC_CHILD: &str = "panic";

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

    struct PanicApp;

    impl ApplicationHandler for PanicApp {
        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            panic!("intentional application panic");
        }
    }

    pub fn run() {
        if let Ok(child) = std::env::var(CHILD) {
            match child.as_str() {
                WAKE_CHILD => wake_child(),
                PANIC_CHILD => panic_child(),
                _ => panic!("unknown child mode: {child}"),
            }
            return;
        }

        run_child(WAKE_CHILD);
        run_child(PANIC_CHILD);
    }

    fn run_child(mode: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .env(CHILD, mode)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "{mode} child failed: {status}");
                return;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("{mode} child timed out");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wake_child() {
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

    fn panic_child() {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut event_loop = EventLoop::new().unwrap();
            let _ = event_loop.run_app_external(PanicApp, LocalSet::new());
        }));
        assert_eq!(
            result
                .expect_err("application panic was not resumed")
                .downcast_ref::<&str>(),
            Some(&"intentional application panic")
        );
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();
}
