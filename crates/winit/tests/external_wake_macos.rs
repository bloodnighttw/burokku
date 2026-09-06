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
    use burokku_winit::{
        raw_window_handle::{HasWindowHandle, RawWindowHandle},
        ElementState, LogicalSize, MouseButton, PhysicalPosition, Window, WindowAttributes,
        WindowEvent, WindowId,
    };
    use objc2_app_kit::{
        NSEvent, NSEventModifierFlags, NSEventType, NSTrackingAreaOptions, NSView,
    };
    use objc2_foundation::{NSPoint, NSSize};
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
                "mouse" => mouse_child(),
                _ => panic!("unknown child mode: {child}"),
            }
            return;
        }

        run_child(WAKE_CHILD);
        run_child(PANIC_CHILD);
        run_child("mouse");
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

    #[derive(Default)]
    struct MouseApp {
        window: Option<Window>,
        received: Vec<WindowEvent>,
        expected: Vec<WindowEvent>,
    }

    impl ApplicationHandler for MouseApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = event_loop
                .create_window(
                    WindowAttributes::default().with_inner_size(LogicalSize::new(200.0, 150.0)),
                )
                .unwrap();
            let RawWindowHandle::AppKit(handle) = window.window_handle().unwrap().as_raw() else {
                panic!("expected AppKit window");
            };
            // SAFETY: The Window retains this NSView and this callback runs on the main thread.
            let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
            // Give the view deterministic geometry even without a WindowServer connection.
            view.setFrameSize(NSSize::new(200.0, 150.0));
            let native_window = view.window().unwrap();
            for event_type in [
                NSEventType::LeftMouseDown,
                NSEventType::LeftMouseDragged,
                NSEventType::LeftMouseUp,
                NSEventType::RightMouseDown,
                NSEventType::RightMouseDragged,
                NSEventType::RightMouseUp,
                NSEventType::OtherMouseDown,
                NSEventType::OtherMouseDragged,
                NSEventType::OtherMouseUp,
                NSEventType::MouseMoved,
            ] {
                let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    event_type,
                    view.convertPoint_toView(NSPoint::new(25.0, 40.0), None),
                    NSEventModifierFlags(0),
                    0.0,
                    native_window.windowNumber(),
                    None,
                    0,
                    1,
                    1.0,
                ).unwrap();
                let buttons = NSEvent::pressedMouseButtons() as u16;
                let position = PhysicalPosition::new(
                    25.0 * window.scale_factor(),
                    110.0 * window.scale_factor(),
                );
                // This NSEvent factory always gives buttonNumber=0, including right/other
                // event types. Numeric button mapping is covered by the backend unit test.
                assert_eq!(event.buttonNumber(), 0);
                let input = match event_type {
                    NSEventType::LeftMouseDown
                    | NSEventType::RightMouseDown
                    | NSEventType::OtherMouseDown => {
                        Some((ElementState::Pressed, MouseButton::Left, 1))
                    }
                    NSEventType::LeftMouseUp
                    | NSEventType::RightMouseUp
                    | NSEventType::OtherMouseUp => {
                        Some((ElementState::Released, MouseButton::Left, 1))
                    }
                    _ => None,
                };
                self.expected.push(match input {
                    Some((state, button, mask)) => WindowEvent::MouseInput {
                        state,
                        button,
                        position,
                        buttons: if state == ElementState::Pressed {
                            buttons | mask
                        } else {
                            buttons & !mask
                        },
                    },
                    None => WindowEvent::CursorMoved { position, buttons },
                });
                // Invoke AppKit's responder selectors without requiring OS input injection.
                match event_type {
                    NSEventType::LeftMouseDown => view.mouseDown(&event),
                    NSEventType::LeftMouseUp => view.mouseUp(&event),
                    NSEventType::LeftMouseDragged => view.mouseDragged(&event),
                    NSEventType::RightMouseDown => view.rightMouseDown(&event),
                    NSEventType::RightMouseUp => view.rightMouseUp(&event),
                    NSEventType::RightMouseDragged => view.rightMouseDragged(&event),
                    NSEventType::OtherMouseDown => view.otherMouseDown(&event),
                    NSEventType::OtherMouseUp => view.otherMouseUp(&event),
                    NSEventType::OtherMouseDragged => view.otherMouseDragged(&event),
                    NSEventType::MouseMoved => view.mouseMoved(&event),
                    _ => unreachable!(),
                }
            }
            let areas = view.trackingAreas();
            assert_eq!(areas.len(), 1);
            assert!(areas.objectAtIndex(0).options().contains(
                NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::InVisibleRect
                    | NSTrackingAreaOptions::EnabledDuringMouseDrag,
            ));
            for (event_type, point) in [
                (NSEventType::MouseEntered, NSPoint::new(25.0, 40.0)),
                (NSEventType::MouseExited, NSPoint::new(-5.0, 40.0)),
            ] {
                // SAFETY: No user data is attached to these synthetic tracking events.
                let event = unsafe {
                    NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
                        event_type, view.convertPoint_toView(point, None), NSEventModifierFlags(0),
                        0.0, native_window.windowNumber(), None, 0, 0, std::ptr::null_mut(),
                    )
                }.unwrap();
                let position = PhysicalPosition::new(
                    point.x * window.scale_factor(),
                    110.0 * window.scale_factor(),
                );
                let buttons = NSEvent::pressedMouseButtons() as u16;
                if event_type == NSEventType::MouseEntered {
                    self.expected
                        .push(WindowEvent::CursorEntered { position, buttons });
                    view.mouseEntered(&event);
                } else {
                    self.expected
                        .push(WindowEvent::CursorLeft { position, buttons });
                    view.mouseExited(&event);
                }
            }
            self.window = Some(window);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: WindowId,
            event: WindowEvent,
        ) {
            if matches!(
                event,
                WindowEvent::MouseInput { .. }
                    | WindowEvent::CursorMoved { .. }
                    | WindowEvent::CursorEntered { .. }
                    | WindowEvent::CursorLeft { .. }
            ) {
                assert_eq!(window_id, self.window.as_ref().unwrap().id());
                self.received.push(event);
                if self.received.len() == self.expected.len() {
                    event_loop.exit();
                }
            }
        }
    }

    fn mouse_child() {
        let app = EventLoop::new()
            .unwrap()
            .run_app_external(MouseApp::default(), LocalSet::new())
            .unwrap();
        assert_eq!(app.expected.len(), 12);
        assert_eq!(app.received, app.expected);
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
