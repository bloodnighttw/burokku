//! macOS implementation backed by AppKit.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    ffi::c_void,
    fmt,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
    ptr::{self, NonNull},
    rc::Rc,
    sync::{atomic::Ordering, Arc, Mutex, Weak},
    time::Instant,
};

use crate::{event_loop::EventLoopWaker, window::WindowState};
use crate::{Error, LogicalSize, PhysicalSize, Window, WindowAttributes, WindowEvent, WindowId};
use core_foundation_sys::{
    base::{kCFAllocatorDefault, CFRelease},
    date::CFAbsoluteTimeGetCurrent,
    runloop::*,
};
use dispatch2::MainThreadBound;
use objc2::{
    define_class, msg_send,
    rc::{autoreleasepool, Retained},
    runtime::ProtocolObject,
    sel, DefinedClass, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent,
    NSEventModifierFlags, NSEventSubtype, NSEventTrackingRunLoopMode, NSEventType, NSView,
    NSViewFrameDidChangeNotification, NSViewLayerContentsRedrawPolicy, NSWindow, NSWindowDelegate,
    NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint,
    NSRect, NSSize, NSString,
};
use objc2_quartz_core::{kCAGravityTopLeft, CALayer, CAMetalLayer};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, WindowHandle,
};

use super::PlatformTick;

const DORMANT_TIMER_INTERVAL: f64 = 86_400.0;

struct PlatformWakeState {
    run_loop: usize,
    // hold the pointer to the CFRunLoopSourceRef
    source: Mutex<usize>,
}

/// Thread-safe bridge from Tokio producers to the AppKit-owned run loop.
#[derive(Clone)]
pub(crate) struct PlatformWake(Arc<PlatformWakeState>);

impl PlatformWake {
    fn new(run_loop: CFRunLoopRef) -> Self {
        Self(Arc::new(PlatformWakeState {
            run_loop: run_loop as usize,
            source: Mutex::new(0),
        }))
    }

    pub(crate) fn wake_up(&self) {
        let source_guard = self
            .0
            .source
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let source = *source_guard as CFRunLoopSourceRef;
        let run_loop = self.0.run_loop as CFRunLoopRef;
        if !source.is_null() && !run_loop.is_null() {
            // SAFETY: Signalling is serialized with clear_source, and
            // run_external retains the source until after it is cleared.
            unsafe {
                CFRunLoopSourceSignal(source);
                CFRunLoopWakeUp(run_loop);
            }
        }
    }

    fn set_source(&self, source: CFRunLoopSourceRef) {
        *self
            .0
            .source
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = source as usize;
    }

    fn clear_source(&self) {
        *self
            .0
            .source
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = 0;
    }
}

impl fmt::Debug for PlatformWake {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PlatformWake")
    }
}

struct ExternalLoopState<F> {
    app: Retained<NSApplication>,
    tick: RefCell<F>,
    wake: PlatformWake,
    timer: Cell<CFRunLoopTimerRef>,
    in_tick: Cell<bool>,
    retick_pending: Cell<bool>,
    panic: RefCell<Option<Box<dyn std::any::Any + Send>>>,
}

impl<F: FnMut() -> PlatformTick> ExternalLoopState<F> {
    fn tick(&self) {
        if self.panic.borrow().is_some() {
            self.app.stop(None);
            return;
        }

        // AppKit can nest its run loop during tracking. Record one deferred
        // tick without re-signalling inside the nested loop, which could spin.
        if self.in_tick.replace(true) {
            self.retick_pending.set(true);
            return;
        }

        // Never let an application/runtime panic cross the extern "C" callback.
        let result = catch_unwind(AssertUnwindSafe(|| (self.tick.borrow_mut())()));
        self.in_tick.set(false);
        let result = match result {
            Ok(result) => result,
            Err(panic) => {
                *self.panic.borrow_mut() = Some(panic);
                self.app.stop(None);
                return;
            }
        };

        let fire = result
            .next_deadline
            .map(|deadline| unsafe {
                CFAbsoluteTimeGetCurrent()
                    + deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64()
            })
            .unwrap_or_else(|| unsafe { CFAbsoluteTimeGetCurrent() } + DORMANT_TIMER_INTERVAL);
        let timer = self.timer.get();
        if !timer.is_null() {
            unsafe { CFRunLoopTimerSetNextFireDate(timer, fire) };
        }

        if result.exit {
            // AppKit applies `stop` after processing another application event.
            self.app.stop(None);
            let event =
                NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                    NSEventType::ApplicationDefined,
                    NSPoint::new(0.0, 0.0),
                    NSEventModifierFlags(0),
                    0.0,
                    0,
                    None,
                    NSEventSubtype::WindowExposed.0,
                    0,
                    0,
                )
            .expect("failed to create AppKit stop event");
            self.app.postEvent_atStart(&event, true);
        } else if self.retick_pending.replace(false) {
            self.wake.wake_up();
        }
    }
}

struct ExternalCallbackGuard {
    run_loop: CFRunLoopRef,
    source: CFRunLoopSourceRef,
    timer: CFRunLoopTimerRef,
    wake: PlatformWake,
    installed: bool,
}

impl ExternalCallbackGuard {
    fn deactivate(&mut self) {
        self.wake.clear_source();
        if !self.installed {
            return;
        }
        self.installed = false;

        unsafe {
            CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopCommonModes);
            CFRunLoopRemoveTimer(self.run_loop, self.timer, kCFRunLoopCommonModes);
            CFRunLoopRemoveSource(self.run_loop, self.source, tracking_run_loop_mode());
            CFRunLoopRemoveTimer(self.run_loop, self.timer, tracking_run_loop_mode());
            CFRunLoopSourceInvalidate(self.source);
            CFRunLoopTimerInvalidate(self.timer);
        }
    }
}

impl Drop for ExternalCallbackGuard {
    fn drop(&mut self) {
        self.deactivate();
        unsafe {
            if !self.source.is_null() {
                CFRelease(self.source.cast());
            }
            if !self.timer.is_null() {
                CFRelease(self.timer.cast());
            }
        }
    }
}

extern "C" fn external_source_callback<F: FnMut() -> PlatformTick>(info: *const c_void) {
    // SAFETY: The source is invalidated before its boxed state is dropped.
    unsafe { (&*info.cast::<ExternalLoopState<F>>()).tick() };
}

extern "C" fn external_timer_callback<F: FnMut() -> PlatformTick>(
    _timer: CFRunLoopTimerRef,
    info: *mut c_void,
) {
    // SAFETY: The timer is invalidated before its boxed state is dropped.
    unsafe { (&*info.cast::<ExternalLoopState<F>>()).tick() };
}

unsafe fn tracking_run_loop_mode() -> core_foundation_sys::string::CFStringRef {
    NSEventTrackingRunLoopMode as *const _ as *const _
}

unsafe fn install_external_callbacks(
    run_loop: CFRunLoopRef,
    source: CFRunLoopSourceRef,
    timer: CFRunLoopTimerRef,
) {
    CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
    CFRunLoopAddTimer(run_loop, timer, kCFRunLoopCommonModes);
    // AppKit enters this nested mode during live resize.
    CFRunLoopAddSource(run_loop, source, tracking_run_loop_mode());
    CFRunLoopAddTimer(run_loop, timer, tracking_run_loop_mode());
}

#[derive(Debug)]
struct NativeEvent {
    window_id: WindowId,
    event: WindowEvent,
}

#[derive(Debug)]
struct WindowDelegateIvars {
    state: Arc<WindowState>,
    dispatcher: Rc<EventDispatcher>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the generated
    // dealloc implementation correctly drops WindowDelegateIvars.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = WindowDelegateIvars]
    struct WindowDelegate;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for WindowDelegate {}

    // SAFETY: All method signatures match NSWindowDelegate and this class is
    // restricted to the main thread.
    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _window: &NSWindow) -> bool {
            self.send(WindowEvent::CloseRequested);
            // Closing is controlled by the Rust handler, just like winit.
            false
        }

        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing_properties(&self, notification: &NSNotification) {
            if let Some(window) = notification_window(notification) {
                let scale_factor = window.backingScaleFactor();
                let new_inner_size = window
                    .contentView()
                    .map(|view| physical_view_size(&view, scale_factor))
                    .unwrap_or_else(|| self.ivars().state.size());
                self.ivars().state.set_scale_factor(scale_factor);
                self.ivars().state.set_size(new_inner_size);
                self.send(WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    new_inner_size,
                });
            }
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn window_did_become_key(&self, _notification: &NSNotification) {
            self.send(WindowEvent::Focused(true));
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _notification: &NSNotification) {
            self.send(WindowEvent::Focused(false));
        }

        #[unsafe(method(windowDidChangeOcclusionState:))]
        fn window_did_change_occlusion_state(&self, notification: &NSNotification) {
            if let Some(window) = notification_window(notification) {
                let visible = window
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible);
                self.send(WindowEvent::Occluded(!visible));
            }
        }
    }
);

impl WindowDelegate {
    fn new(
        mtm: MainThreadMarker,
        state: Arc<WindowState>,
        dispatcher: Rc<EventDispatcher>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(WindowDelegateIvars { state, dispatcher });
        // SAFETY: NSObject's init has the declared signature and the ivars were
        // initialized above.
        unsafe { msg_send![super(this), init] }
    }

    fn send(&self, event: WindowEvent) {
        self.ivars().dispatcher.dispatch(NativeEvent {
            window_id: self.ivars().state.id,
            event,
        });
    }
}

#[derive(Debug)]
struct ContentViewIvars {
    state: Arc<WindowState>,
    dispatcher: Rc<EventDispatcher>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing. ContentView is confined to
    // the AppKit main thread and its generated dealloc drops the Rust ivars.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ContentViewIvars]
    struct ContentView;

    // SAFETY: NSObjectProtocol has no additional safety requirements.
    unsafe impl NSObjectProtocol for ContentView {}

    impl ContentView {
        #[unsafe(method(frameDidChange:))]
        fn frame_did_change(&self, _notification: &NSNotification) {
            let scale_factor = self
                .window()
                .map(|window| window.backingScaleFactor())
                .unwrap_or_else(|| self.ivars().state.scale_factor());
            let size = physical_view_size(self, scale_factor);

            // raw-window-metal normally installs a CAMetalLayer sublayer with
            // resize gravity. Keep the next drawable at the new physical size
            // immediately, and do not stretch the last presented drawable
            // while the coalesced redraw is still pending.
            self.sync_metal_surface(size);

            if size != self.ivars().state.size() {
                self.ivars().state.set_size(size);
                self.send(WindowEvent::Resized(size));
            }

            // AppKit coalesces these invalidations and calls drawRect: at a
            // display-safe point, including from its nested live-resize loop.
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty_rect: NSRect) {
            self.sync_metal_surface(self.ivars().state.size());
            self.ivars()
                .state
                .redraw_requested
                .store(false, Ordering::Release);
            self.send(WindowEvent::RedrawRequested);

            // SAFETY: The selector and argument exactly match NSView's
            // drawRect: method.
            unsafe {
                let _: () = msg_send![super(self), drawRect: dirty_rect];
            }
        }
    }
);

impl ContentView {
    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        state: Arc<WindowState>,
        dispatcher: Rc<EventDispatcher>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ContentViewIvars { state, dispatcher });
        // SAFETY: The selector and argument exactly match NSView's designated
        // frame initializer, and the Rust ivars were initialized above.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };

        this.setWantsLayer(true);
        this.setPostsFrameChangedNotifications(true);

        let notification_center = NSNotificationCenter::defaultCenter();
        // SAFETY: frameDidChange: is implemented above with the notification
        // signature, and the observer/object live together as the same view.
        unsafe {
            notification_center.addObserver_selector_name_object(
                &this,
                sel!(frameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&this),
            );
        }

        this
    }

    fn send(&self, event: WindowEvent) {
        self.ivars().dispatcher.dispatch(NativeEvent {
            window_id: self.ivars().state.id,
            event,
        });
    }

    fn sync_metal_surface(&self, size: PhysicalSize<u32>) {
        let Some(root_layer) = self.layer() else {
            return;
        };

        sync_metal_layer(&root_layer, size);

        // SAFETY: Reading an AppKit-owned layer tree is safe on the main
        // thread; ContentView is main-thread-only.
        if let Some(sublayers) = unsafe { root_layer.sublayers() } {
            for layer in sublayers.iter() {
                sync_metal_layer(&layer, size);
            }
        }
    }
}

fn sync_metal_layer(layer: &CALayer, size: PhysicalSize<u32>) {
    let Some(layer) = layer.downcast_ref::<CAMetalLayer>() else {
        return;
    };

    layer.setDrawableSize(NSSize::new(size.width as f64, size.height as f64));
    // SAFETY: This QuartzCore constant is initialized by the framework before
    // an AppKit view can create a layer.
    layer.setContentsGravity(unsafe { kCAGravityTopLeft });
}

type EventHandler = Box<dyn FnMut(WindowId, WindowEvent)>;

#[derive(Default)]
struct EventDispatcher {
    handler: RefCell<Option<EventHandler>>,
    pending: RefCell<VecDeque<NativeEvent>>,
}

impl std::fmt::Debug for EventDispatcher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EventDispatcher")
            .field("has_handler", &self.handler.borrow().is_some())
            .field("pending_events", &self.pending.borrow().len())
            .finish()
    }
}

impl EventDispatcher {
    fn set_handler(&self, handler: EventHandler) {
        *self.handler.borrow_mut() = Some(handler);

        let pending = self.pending.borrow_mut().drain(..).collect::<Vec<_>>();
        for event in pending {
            self.dispatch(event);
        }
    }

    fn clear_handler(&self) {
        self.handler.borrow_mut().take();
    }

    fn dispatch(&self, event: NativeEvent) {
        let Some(mut handler) = self.handler.borrow_mut().take() else {
            self.pending.borrow_mut().push_back(event);
            return;
        };

        handler(event.window_id, event.event);
        loop {
            // Release the pending queue borrow before application code runs;
            // the handler can synchronously trigger another AppKit callback.
            let event = self.pending.borrow_mut().pop_front();
            let Some(event) = event else {
                break;
            };
            handler(event.window_id, event.event);
        }

        *self.handler.borrow_mut() = Some(handler);
    }
}

fn notification_window(notification: &NSNotification) -> Option<Retained<NSWindow>> {
    // NSWindow delegate notifications expose an Objective-C object, which is
    // dynamically checked before use.
    notification.object()?.downcast::<NSWindow>().ok()
}

fn physical_view_size(view: &NSView, scale: f64) -> PhysicalSize<u32> {
    let logical = view.frame().size;
    PhysicalSize::new(
        physical_dimension(logical.width, scale),
        physical_dimension(logical.height, scale),
    )
}

fn physical_dimension(points: f64, scale: f64) -> u32 {
    (points * scale).round().clamp(0.0, u32::MAX as f64) as u32
}

struct PlatformWindowInner {
    window: Retained<NSWindow>,
    view: Retained<ContentView>,
    // NSWindow's delegate property is weak, so Rust must retain it.
    _delegate: Retained<WindowDelegate>,
}

pub(crate) struct PlatformWindow {
    inner: MainThreadBound<PlatformWindowInner>,
}

impl PlatformWindow {
    fn new(
        mtm: MainThreadMarker,
        window: Retained<NSWindow>,
        view: Retained<ContentView>,
        delegate: Retained<WindowDelegate>,
    ) -> Self {
        Self {
            inner: MainThreadBound::new(
                PlatformWindowInner {
                    window,
                    view,
                    _delegate: delegate,
                },
                mtm,
            ),
        }
    }

    pub(crate) fn view_ptr(&self) -> Option<NonNull<std::ffi::c_void>> {
        let mtm = MainThreadMarker::new()?;
        Some(NonNull::from(&*self.inner.get(mtm).view).cast())
    }

    pub(crate) fn set_title(&self, title: &str) {
        self.inner.get_on_main(|inner| {
            inner.window.setTitle(&NSString::from_str(title));
        });
    }

    pub(crate) fn request_redraw(&self) {
        self.inner
            .get_on_main(|inner| inner.view.setNeedsDisplay(true));
    }

    pub(crate) fn set_inner_size(&self, size: LogicalSize<f64>) {
        self.inner.get_on_main(|inner| {
            inner
                .window
                .setContentSize(NSSize::new(size.width, size.height));
        });
    }

    pub(crate) fn close(&self) {
        self.inner.get_on_main(|inner| inner.window.close());
    }
}

impl HasWindowHandle for PlatformWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let view = self.view_ptr().ok_or(HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(view);
        // SAFETY: PlatformWindow retains the NSWindow and its content NSView
        // for at least as long as this borrowed WindowHandle.
        Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for PlatformWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        let handle = AppKitDisplayHandle::new();
        // SAFETY: AppKit has no per-connection display pointer; this empty
        // handle represents the process-wide AppKit display.
        Ok(unsafe { DisplayHandle::borrow_raw(handle.into()) })
    }
}

pub(crate) struct PlatformEventLoop {
    app: Retained<NSApplication>,
    mtm: MainThreadMarker,
    wake: PlatformWake,
    dispatcher: Rc<EventDispatcher>,
    windows: RefCell<HashMap<isize, Weak<WindowState>>>,
    next_window_id: Cell<u64>,
}

impl PlatformEventLoop {
    pub(crate) fn new() -> crate::Result<Self> {
        let mtm = MainThreadMarker::new().ok_or(Error::NotMainThread)?;
        let app = NSApplication::sharedApplication(mtm);
        app.finishLaunching();
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        // Command-line binaries have no application bundle to activate them.
        // Match winit's default behavior and bring the first native Window in
        // front of the launching terminal.
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        Ok(Self {
            app,
            mtm,
            wake: PlatformWake::new(unsafe { CFRunLoopGetMain() }),
            dispatcher: Rc::new(EventDispatcher::default()),
            windows: RefCell::new(HashMap::new()),
            next_window_id: Cell::new(1),
        })
    }

    pub(crate) fn waker(&self) -> PlatformWake {
        self.wake.clone()
    }

    pub(crate) fn create_window(
        &self,
        attributes: WindowAttributes,
        event_loop_waker: EventLoopWaker,
    ) -> crate::Result<Window> {
        if !(attributes.inner_size.width.is_finite()
            && attributes.inner_size.height.is_finite()
            && attributes.inner_size.width > 0.0
            && attributes.inner_size.height > 0.0)
        {
            return Err(Error::WindowCreation(
                "inner size must contain positive finite dimensions".into(),
            ));
        }

        let mut style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        if attributes.resizable {
            style |= NSWindowStyleMask::Resizable;
        }

        // SAFETY: The content rectangle and style flags meet NSWindow's
        // initializer requirements, and creation occurs on the main thread.
        let native_window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(self.mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(attributes.inner_size.width, attributes.inner_size.height),
                ),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: Windows created without an NSWindowController must disable
        // release-on-close while Rust retains the object.
        unsafe { native_window.setReleasedWhenClosed(false) };

        let id = WindowId(self.next_window_id.get());
        self.next_window_id
            .set(self.next_window_id.get().wrapping_add(1).max(1));
        let scale_factor = native_window.backingScaleFactor();
        let state = Arc::new(WindowState::new(
            id,
            PhysicalSize::new(
                physical_dimension(attributes.inner_size.width, scale_factor),
                physical_dimension(attributes.inner_size.height, scale_factor),
            ),
            scale_factor,
            event_loop_waker,
        ));

        native_window.setTitle(&NSString::from_str(&attributes.title));
        native_window.setAcceptsMouseMovedEvents(true);
        // A GPU-backed view cannot benefit from AppKit copying and stretching
        // cached view contents during a live resize.
        native_window.setPreservesContentDuringLiveResize(false);
        let view = ContentView::new(
            self.mtm,
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(attributes.inner_size.width, attributes.inner_size.height),
            ),
            state.clone(),
            self.dispatcher.clone(),
        );
        view.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::DuringViewResize);
        native_window.setContentView(Some(&view));

        let delegate = WindowDelegate::new(self.mtm, state.clone(), self.dispatcher.clone());
        native_window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        native_window.center();
        #[allow(deprecated)]
        self.app.activateIgnoringOtherApps(true);
        native_window.makeKeyAndOrderFront(None);

        self.windows
            .borrow_mut()
            .insert(native_window.windowNumber(), Arc::downgrade(&state));

        Ok(Window {
            state,
            platform: PlatformWindow::new(self.mtm, native_window, view, delegate),
            _thread_affinity: std::marker::PhantomData,
        })
    }

    pub(crate) fn set_handler(&self, mut handler: impl FnMut(WindowId, WindowEvent) + 'static) {
        let wake = self.wake.clone();
        self.dispatcher
            .set_handler(Box::new(move |window_id, event| {
                handler(window_id, event);
                // A platform-owned loop must run about_to_wait after native
                // dispatch even when no Tokio task independently wakes it.
                wake.wake_up();
            }));
    }

    pub(crate) fn clear_handler(&self) {
        self.dispatcher.clear_handler();
    }

    pub(crate) fn flush_windows(&self) {
        autoreleasepool(|_| self.app.updateWindows());
    }

    pub(crate) fn run_external<F>(&self, tick: F, shutdown: impl FnOnce()) -> crate::Result<()>
    where
        F: FnMut() -> PlatformTick,
    {
        let run_loop = unsafe { CFRunLoopGetMain() };
        let mut state = Box::new(ExternalLoopState {
            app: self.app.clone(),
            tick: RefCell::new(tick),
            wake: self.wake.clone(),
            timer: Cell::new(ptr::null_mut()),
            in_tick: Cell::new(false),
            retick_pending: Cell::new(false),
            panic: RefCell::new(None),
        });
        let info = (&mut *state as *mut ExternalLoopState<_>).cast::<c_void>();
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
            perform: external_source_callback::<F>,
        };
        let source = unsafe { CFRunLoopSourceCreate(kCFAllocatorDefault, 0, &mut source_context) };
        assert!(
            !source.is_null(),
            "failed to create the external run-loop source"
        );
        let mut callbacks = ExternalCallbackGuard {
            run_loop,
            source,
            timer: ptr::null_mut(),
            wake: self.wake.clone(),
            installed: false,
        };

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
                CFAbsoluteTimeGetCurrent() + DORMANT_TIMER_INTERVAL,
                DORMANT_TIMER_INTERVAL,
                0,
                0,
                external_timer_callback::<F>,
                &mut timer_context,
            )
        };
        assert!(
            !timer.is_null(),
            "failed to create the external run-loop timer"
        );
        callbacks.timer = timer;
        state.timer.set(timer);
        unsafe { install_external_callbacks(run_loop, source, timer) };
        callbacks.installed = true;
        self.wake.set_source(source);

        self.wake.wake_up();
        let app_panic = catch_unwind(AssertUnwindSafe(|| self.app.run())).err();

        // Prevent callbacks before dropping arbitrary user futures. The guard
        // still retains both CF objects until runtime wake producers stop.
        callbacks.deactivate();
        let shutdown_panic = catch_unwind(AssertUnwindSafe(shutdown)).err();
        let callback_panic = state.panic.borrow_mut().take();
        drop(callbacks);
        drop(state);

        if let Some(panic) = callback_panic.or(app_panic).or(shutdown_panic) {
            resume_unwind(panic);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegate_events_dispatch_immediately_when_handler_is_installed() {
        let dispatcher = EventDispatcher::default();
        let received = Rc::new(RefCell::new(Vec::new()));
        dispatcher.set_handler(Box::new({
            let received = received.clone();
            move |window_id, event| received.borrow_mut().push((window_id, event))
        }));

        dispatcher.dispatch(NativeEvent {
            window_id: WindowId(7),
            event: WindowEvent::Resized(PhysicalSize::new(1024, 768)),
        });

        assert_eq!(
            *received.borrow(),
            [(
                WindowId(7),
                WindowEvent::Resized(PhysicalSize::new(1024, 768))
            )]
        );
    }

    #[test]
    fn nested_native_events_dispatch_after_the_active_handler_returns() {
        let dispatcher = Rc::new(EventDispatcher::default());
        let received = Rc::new(RefCell::new(Vec::new()));
        dispatcher.set_handler(Box::new({
            let dispatcher = Rc::clone(&dispatcher);
            let received = Rc::clone(&received);
            move |window_id, event| {
                let emit_nested = matches!(event, WindowEvent::Focused(true));
                received.borrow_mut().push((window_id, event));
                if emit_nested {
                    dispatcher.dispatch(NativeEvent {
                        window_id,
                        event: WindowEvent::Resized(PhysicalSize::new(1024, 768)),
                    });
                }
            }
        }));

        dispatcher.dispatch(NativeEvent {
            window_id: WindowId(7),
            event: WindowEvent::Focused(true),
        });

        assert_eq!(
            *received.borrow(),
            [
                (WindowId(7), WindowEvent::Focused(true)),
                (
                    WindowId(7),
                    WindowEvent::Resized(PhysicalSize::new(1024, 768))
                )
            ]
        );
    }
}
