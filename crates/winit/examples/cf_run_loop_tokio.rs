//! AppKit-owned main loop driving a patched Tokio current-thread runtime.
//!
//! Run on macOS, then continuously live-resize the window. LLRT executes
//! JavaScript that increments a counter every second and fetches example.com
//! every three seconds. Both continue because the CF source and timer are
//! installed in `NSEventTrackingRunLoopMode` as well as common modes.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("this example requires macOS");
}

#[cfg(target_os = "macos")]
mod appkit {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease};
    use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
    use core_foundation_sys::runloop::*;
    use llrt_utils::primordials::{BasePrimordials, Primordial};
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType,
        NSEventTrackingRunLoopMode, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
    use tokio::runtime::{Builder, ExternalWake, Runtime};
    use tokio::task::LocalSet;

    const LLRT_SCRIPT: &str = include_str!("cf_run_loop_tokio.js");

    fn install_llrt_globals(context: &runtime::rquickjs::Ctx<'_>) -> runtime::Result<()> {
        BasePrimordials::init(context)?;
        let (_, _, globals) = llrt_modules::module_builder::ModuleBuilder::default().build();
        globals.attach(context)
    }

    struct Wake {
        run_loop: AtomicUsize,
        source: AtomicUsize,
    }

    impl Wake {
        fn signal(&self) {
            let source = self.source.load(Ordering::Acquire) as CFRunLoopSourceRef;
            let run_loop = self.run_loop.load(Ordering::Acquire) as CFRunLoopRef;
            if !source.is_null() && !run_loop.is_null() {
                // SAFETY: retained until NSApplication::run returns.
                unsafe {
                    CFRunLoopSourceSignal(source);
                    CFRunLoopWakeUp(run_loop);
                }
            }
        }
    }

    impl ExternalWake for Wake {
        fn wake(&self) {
            self.signal();
        }
    }

    struct State {
        runtime: RefCell<Option<Runtime>>,
        local: RefCell<Option<LocalSet>>,
        wake: Arc<Wake>,
        timer: Cell<CFRunLoopTimerRef>,
        in_tick: Cell<bool>,
    }

    impl State {
        fn tick(&self) {
            // AppKit can nest its run loop during tracking. Defer recursive ticks.
            if self.in_tick.replace(true) {
                self.wake.signal();
                return;
            }
            let result = {
                let runtime = self.runtime.borrow();
                let mut local = self.local.borrow_mut();
                let (Some(runtime), Some(local)) = (runtime.as_ref(), local.as_mut()) else {
                    self.in_tick.set(false);
                    return;
                };
                runtime.tick_nonblocking_with_local_set(local)
            };
            self.in_tick.set(false);

            // The runtime re-signals the CF source when a bounded tick leaves
            // runnable work, including during AppKit's live-resize loop.
            let fire = result
                .next_deadline
                .map(|deadline| unsafe {
                    CFAbsoluteTimeGetCurrent()
                        + deadline
                            .saturating_duration_since(Instant::now())
                            .as_secs_f64()
                })
                .unwrap_or_else(|| unsafe { CFAbsoluteTimeGetCurrent() } + 86_400.0);
            if !self.timer.get().is_null() {
                unsafe { CFRunLoopTimerSetNextFireDate(self.timer.get(), fire) };
            }
        }
    }

    extern "C" fn source_callback(info: *const c_void) {
        unsafe { (&*info.cast::<State>()).tick() };
    }

    extern "C" fn timer_callback(_: CFRunLoopTimerRef, info: *mut c_void) {
        unsafe { (&*info.cast::<State>()).tick() };
    }

    unsafe fn tracking_mode() -> core_foundation_sys::string::CFStringRef {
        NSEventTrackingRunLoopMode as *const _ as *const _
    }

    unsafe fn install(
        run_loop: CFRunLoopRef,
        source: CFRunLoopSourceRef,
        timer: CFRunLoopTimerRef,
    ) {
        CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
        CFRunLoopAddTimer(run_loop, timer, kCFRunLoopCommonModes);
        // Live resize runs in this nested AppKit mode.
        CFRunLoopAddSource(run_loop, source, tracking_mode());
        CFRunLoopAddTimer(run_loop, timer, tracking_mode());
    }

    pub fn main() {
        let mtm = MainThreadMarker::new().expect("must run on the process main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.finishLaunching();

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(720.0, 420.0)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(
            "LLRT + Tokio external loop — live-resize for at least five seconds",
        ));
        window.center();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        let run_loop = unsafe { CFRunLoopGetMain() };
        let wake = Arc::new(Wake {
            run_loop: AtomicUsize::new(run_loop as usize),
            source: AtomicUsize::new(0),
        });
        let runtime = Builder::new_current_thread()
            .enable_all()
            .external_event_loop(wake.clone())
            .external_tick_budget(64)
            .build()
            .unwrap();
        let main_thread = std::thread::current().id();
        let local = LocalSet::new();
        local.spawn_local(async move {
            assert_eq!(std::thread::current().id(), main_thread);
            let (javascript, driver) = match runtime::Runtime::builder()
                .plugin(install_llrt_globals)
                .build_driven()
                .await
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("failed to initialize the LLRT-backed runtime: {error}");
                    return;
                }
            };
            let driver = tokio::task::spawn_local(driver.run());

            if let Err(error) = javascript.eval::<()>(LLRT_SCRIPT).await {
                eprintln!("failed to evaluate the LLRT test script: {error}");
                return;
            }
            assert_eq!(std::thread::current().id(), main_thread);

            match driver.await {
                Ok(()) => eprintln!("LLRT JavaScript driver stopped unexpectedly"),
                Err(error) => eprintln!("LLRT JavaScript driver failed: {error}"),
            }
        });

        // Opt in to the expected limitation:
        // TOKIO_EXTERNAL_CPU_BLOCK=1 cargo run -p burokku-winit --example cf_run_loop_tokio
        if std::env::var_os("TOKIO_EXTERNAL_CPU_BLOCK").is_some() {
            runtime.spawn(async {
                tokio::time::sleep(Duration::from_secs(3)).await;
                println!("starting expected two-second main-thread blockage");
                let end = Instant::now() + Duration::from_secs(2);
                while Instant::now() < end {
                    std::hint::spin_loop();
                }
                println!("main-thread blockage ended");
            });
        }

        let mut state = Box::new(State {
            runtime: RefCell::new(Some(runtime)),
            local: RefCell::new(Some(local)),
            wake: wake.clone(),
            timer: Cell::new(ptr::null_mut()),
            in_tick: Cell::new(false),
        });
        let info = (&mut *state as *mut State).cast::<c_void>();
        let mut source_context = CFRunLoopSourceContext {
            version: 0,
            info,
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: source_callback,
        };
        let source = unsafe { CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &mut source_context) };
        assert!(!source.is_null());
        wake.source.store(source as usize, Ordering::Release);

        let mut timer_context = CFRunLoopTimerContext {
            version: 0,
            info,
            retain: None,
            release: None,
            copyDescription: None,
        };
        let timer = unsafe {
            CFRunLoopTimerCreate(
                kCFAllocatorDefault,
                CFAbsoluteTimeGetCurrent() + 86_400.0,
                86_400.0,
                0,
                0,
                timer_callback,
                &mut timer_context,
            )
        };
        assert!(!timer.is_null());
        state.timer.set(timer);
        unsafe { install(run_loop, source, timer) };

        wake.signal();
        app.run();

        wake.source.store(0, Ordering::Release);
        // Cancel LLRT, then stop and join Tokio's reactor while the source and
        // State context are still retained. A producer that loaded the old
        // source pointer before the atomic clear can therefore signal it safely.
        drop(state.local.borrow_mut().take());
        drop(state.runtime.borrow_mut().take());
        unsafe {
            CFRunLoopRemoveSource(run_loop, source, kCFRunLoopCommonModes);
            CFRunLoopRemoveTimer(run_loop, timer, kCFRunLoopCommonModes);
            CFRunLoopRemoveSource(run_loop, source, tracking_mode());
            CFRunLoopRemoveTimer(run_loop, timer, tracking_mode());
            CFRunLoopSourceInvalidate(source);
            CFRunLoopTimerInvalidate(timer);
            CFRelease(source.cast());
            CFRelease(timer.cast());
        }
        drop(state); // Source/timer callbacks can no longer observe this context.
        drop(window);
    }
}

#[cfg(target_os = "macos")]
fn main() {
    appkit::main();
}
