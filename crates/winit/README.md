# burokku-winit

`burokku-winit` is a deliberately small native windowing crate for Burokku. It
provides a winit-inspired application-handler API, raw window/display handles
for renderers such as `wgpu`, and an event loop that can drive Tokio without
letting Tokio own or block the platform thread.

The crate is currently a focused integration layer rather than a drop-in
replacement for upstream [`winit`](https://crates.io/crates/winit). Only the
API needed by Burokku is implemented, and the native backend currently supports
**macOS only**. Other targets compile but `EventLoop::new()` returns
`Error::UnsupportedPlatform`.

## Why this crate exists

A normal Tokio current-thread runtime owns its outer blocking wait. A native UI
application has the opposite requirement: AppKit must own the process main
thread and its run loop, including nested run loops entered during operations
such as live window resizing.

`EventLoop::run_app_external` joins the two models:

```text
AppKit / CFRunLoop owns the main thread
  -> native wake source or deadline timer fires
  -> burokku-winit performs one bounded, nonblocking Tokio tick
  -> Tokio tasks, timers, I/O readiness, and one persistent LocalSet progress
  -> application callbacks receive queued window events
  -> control returns to AppKit
```

The macOS backend installs a Core Foundation wake source and timer in both the
common run-loop modes and AppKit's event-tracking mode. This allows Tokio and
thread-affine `LocalSet` tasks, including QuickJS/LLRT work, to continue making
progress while a window is being interactively resized.

## Features

- A winit-style `ApplicationHandler` lifecycle.
- Main-thread native window creation and event delivery.
- Logical sizing and physical-pixel size/position types.
- Keyboard, modifier, mouse, focus, occlusion, resize, scale-factor, redraw,
  and close-request events.
- `raw-window-handle` 0.6 support for graphics integrations.
- A thread-safe `EventLoopProxy` for waking an idle UI loop.
- Patched Tokio current-thread runtime integration with configurable bounded
  tick work.
- Persistent `LocalSet` support for `!Send` futures and thread-affine runtimes.
- Responsive redraw and Tokio progress during AppKit's nested live-resize loop.

## Workspace and Tokio setup

This crate's external-loop API depends on the patched Tokio source in
`crates/tokio/tokio`. The repository workspace already selects it in the root
`Cargo.toml`:

```toml
[patch.crates-io]
tokio = { path = "crates/tokio/tokio" }
```

Cargo patches are not transitive. A downstream root project that consumes
`burokku-winit` by path or Git must apply an equivalent `[patch.crates-io]`
entry pointing to this repository's patched Tokio checkout. Without that patch,
methods such as `Builder::external_event_loop`,
`Builder::external_tick_budget`, and
`Runtime::tick_nonblocking_with_local_set` are unavailable.

Use one resolved Tokio version for the application, `burokku-winit`, and any
runtime integration built on top of it. In this workspace, a dependency can use
Tokio normally because the root patch redirects it:

```toml
[dependencies]
burokku-winit = { path = "crates/winit" }
tokio = { version = "1", features = ["macros", "net", "rt", "sync", "time"] }

[patch.crates-io]
tokio = { path = "crates/tokio/tokio" }
```

Adjust the paths when `burokku-winit` is consumed from another repository.

## Minimal application

Create the event loop on the macOS process main thread, build the runtime from
`external_runtime_builder`, and pass one persistent `LocalSet` to
`run_app_external`:

```rust
use burokku_winit::{
    application::ApplicationHandler,
    ActiveEventLoop, EventLoop, LogicalSize, Window, WindowEvent, WindowId,
};
use tokio::task::LocalSet;

#[derive(Default)]
struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title("Burokku")
            .with_inner_size(LogicalSize::new(800.0, 600.0));

        self.window = Some(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );
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

        match event {
            WindowEvent::CloseRequested => {
                // A close request is advisory; the handler decides when to close.
                window.close();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // Render and present here.
                window.pre_present_notify();
            }
            WindowEvent::Resized(_size) => {
                window.request_redraw();
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop = EventLoop::new()?;
    let runtime = event_loop
        .external_runtime_builder()
        .enable_all()
        .external_tick_budget(64)
        .build()?;
    let local_set = LocalSet::new();

    // Spawn thread-affine work before entering the native loop if needed.
    local_set.spawn_local(async {
        // QuickJS/LLRT or other !Send work can live here.
    });

    let _app = event_loop.run_app_external(App::default(), runtime, local_set)?;
    Ok(())
}
```

`run_app_external` blocks until `ActiveEventLoop::exit()` is requested or the
platform loop stops. During teardown it stops the retained `LocalSet` and Tokio
runtime, invokes `ApplicationHandler::exiting`, and returns ownership of the
application value.

## Application lifecycle

`ApplicationHandler` has four callbacks, all invoked on the event-loop thread:

- `resumed` runs before the native loop begins and is the usual place to create
  windows and initialize renderer state.
- `window_event` receives an event together with the originating `WindowId`.
- `about_to_wait` runs after a bounded Tokio tick and before the native loop
  waits again. It is useful for consuming state produced by other threads.
- `exiting` runs once while the event loop is shutting down.

Native events are normally delivered immediately. If an AppKit callback occurs
reentrantly while application code is already mutably borrowed, the event is
deferred and later delivered in FIFO order. This avoids mutable reentrancy while
keeping resize and redraw events responsive in nested platform loops.

## Event-loop control

`ActiveEventLoop` provides the operations normally needed by callbacks:

- `create_window(attributes)` creates and shows a window on the event-loop
  thread.
- `create_proxy()` returns a cloneable, `Send + Sync` wake handle.
- `set_control_flow(...)` changes how the native loop schedules application
  ticks.
- `exit()` requests orderly shutdown.
- `flush_windows()` flushes pending native window ordering before renderer
  initialization when necessary.

The available `ControlFlow` modes are:

- `Wait` (default): sleep until native input, an explicit wake, or Tokio's next
  timer deadline.
- `WaitUntil(instant)`: wake at the earlier of the application deadline and
  Tokio's next timer deadline.
- `Poll`: continually request another event-loop tick. This is useful for
  continuous rendering but consumes more power than redraw-on-demand.

### Waking from another thread

`EventLoopProxy::wake_up` does not synthesize a custom application event. It
only causes a prompt bounded Tokio tick followed by `about_to_wait`, where the
application can inspect a channel, atomic, or other shared state:

```rust
let proxy = event_loop.create_proxy();
std::thread::spawn(move || {
    // Update thread-safe application state first.
    proxy.wake_up();
});
```

The proxy also implements the patched `tokio::runtime::ExternalWake` trait.

## Windows and rendering

Create attributes with `Window::default_attributes()` and the builder methods:

- `with_title(String)`;
- `with_inner_size(LogicalSize<f64>)`;
- `with_resizable(bool)`.

The defaults are a resizable `800 x 600` logical-point window titled
`"Burokku"`.

A `Window` exposes its stable `WindowId`, physical inner size, scale factor,
redraw requests, title and logical-size updates, explicit close, and raw window
and display handles. `set_inner_size` rejects non-finite, zero, and negative
dimensions. `pre_present_notify` is currently a no-op compatibility hook for
renderer code shared with winit.

On macOS the content view is layer-backed. During resize, the backend updates
attached `CAMetalLayer` drawable sizes in physical pixels before issuing
`RedrawRequested`, which avoids stretching an old drawable while waiting for a
new frame.

`Window` is intentionally neither `Send` nor `Sync`: native operations and the
final native release must stay on the platform event-loop thread. Keep windows
and UI/renderer state in the application handler; use `EventLoopProxy` to notify
that thread from workers.

## Window events

The current public event set is:

- `CloseRequested`;
- `Resized(PhysicalSize<u32>)`;
- `ScaleFactorChanged { scale_factor, new_inner_size }`;
- `RedrawRequested`;
- `Focused(bool)` and `Occluded(bool)`;
- `KeyboardInput(KeyEvent)` and `ModifiersChanged(Modifiers)`;
- `CursorMoved { position }`;
- `MouseInput { state, button, position }`;
- `MouseWheel { delta_x, delta_y, precise, position }`.

Keyboard input contains the platform virtual key code, layout-resolved text when
available, pressed/released state, repeat status, and modifier snapshot. Mouse
coordinates and sizes reported by events are physical pixels.

## Runtime behavior and constraints

The external runtime must be built with Tokio's **current-thread** scheduler.
Passing a multi-thread runtime returns `Error::InvalidExternalRuntime`. Prefer
`EventLoop::external_runtime_builder()` because it creates the correct builder
and wires Tokio's external wake callback to the native loop.

Each native callback performs nonblocking reactor work, advances timers, polls
the persistent `LocalSet`, and polls at most the configured number of regular
Tokio scheduler tasks. If runnable work remains, Tokio wakes the platform loop
for another tick. I/O readiness is collected on Tokio's dedicated Mio reactor
thread, but application futures and `LocalSet` futures remain on the UI thread.

The tick budget bounds the number of scheduler tasks polled; it is **not
preemption**. A future that performs long synchronous work in one poll still
blocks AppKit, window events, and all other current-thread tasks. Move blocking
or CPU-heavy work to `spawn_blocking` or another worker and wake the UI loop when
its result is ready.

Additional constraints:

- An `EventLoop` can be run only once; a second call returns `Error::AlreadyRun`.
- On macOS, event-loop creation and driving must happen on the process main
  thread or `Error::NotMainThread` is returned.
- Keep one persistent `LocalSet`; do not alternate different local sets or mix
  external ticks with another driver for the same runtime.
- Tokio paused/virtual time is not supported by native deadline driving.
- `ControlFlow::Poll` can increase CPU and battery use.
- Linux, Windows, Wayland, X11, and WASI native backends are not implemented.

## Example

The full `cf_run_loop_tokio` example runs LLRT/QuickJS work, Tokio timers, and a
network request on the external-loop integration:

```sh
cargo run -p burokku-winit --example cf_run_loop_tokio
```

While it runs, continuously resize the window for at least five seconds. The
JavaScript counter and fetch attempts should continue during the drag, showing
that the wake source and timer are active in AppKit's nested tracking loop.

To demonstrate the non-preemptive limitation, run:

```sh
TOKIO_EXTERNAL_CPU_BLOCK=1 \
  cargo run -p burokku-winit --example cf_run_loop_tokio
```

The example intentionally performs two seconds of synchronous work on the main
thread; both the window and timers are expected to stall for those two seconds.

## Validation

From the repository root:

```sh
cargo test -p burokku-winit
cargo check -p burokku-winit --example cf_run_loop_tokio
```

The patched Tokio driver and runtime integration have additional focused tests:

```sh
cargo test --manifest-path crates/tokio/Cargo.toml -p tokio \
  --test rt_external_driver --features full
cargo test -p runtime --test external_tokio
```

See [`docs/tokio_external_event_loop.md`](../../docs/tokio_external_event_loop.md)
for the patched Tokio scheduler/reactor design, wake and deadline contract,
shutdown invariants, test coverage, and notes for future platform backends.
