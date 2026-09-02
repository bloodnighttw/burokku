# Upstream Tokio external-loop bridge

`burokku-winit` keeps AppKit and `CFRunLoop` in control of the process main
thread while using unmodified Tokio 1.53.1 from crates.io. No Cargo patch,
vendored Tokio source, custom Mio reactor, or fork-only runtime API is required.

## Ownership model

```text
Tokio worker thread                    AppKit main thread
-------------------                    ------------------
I/O and timer drivers                  CFRunLoopSource callback
ordinary `tokio::spawn` tasks  wake    poll persistent LocalSet once
`spawn_blocking` work          ----->  QuickJS / LLRT callbacks
                                       DOM / layout / rendering
```

`EventLoop::run_app_external(application, local_set)` builds and owns a
multi-thread Tokio runtime with exactly one worker and all drivers enabled. The
caller supplies one persistent `LocalSet`, which remains on the main thread for
the native loop's lifetime.

The thread contract is explicit:

- `tokio::task::spawn_local`, QuickJS, DOM, layout, rendering, and application
  callbacks run on the AppKit main thread;
- ordinary `tokio::spawn`, Tokio timers, and I/O readiness run on the Tokio
  worker;
- `spawn_blocking` runs on Tokio's blocking pool;
- native window callbacks run on the AppKit main thread.

A detached worker task that changes UI-observed shared state must either wake an
awaited local future/channel or call `EventLoopProxy::wake_up`. Tokio does not
wake AppKit for unrelated worker-side state changes.

## Poll and wake bridge

Each native tick enters the Tokio runtime, constructs
`LocalSet::run_until(std::future::pending())`, polls that temporary future once,
and drops it. `run_until` is cancel-safe: dropping the temporary future does not
drop tasks owned by the persistent `LocalSet`.

`EventLoopProxy` implements `std::task::Wake`. A `Waker` backed by the proxy is
passed to every `LocalSet` poll. When a timer, socket, channel, worker task, or
LocalSet backlog makes local work runnable, Tokio calls that waker, which
signals the macOS `CFRunLoopSource` and wakes `CFRunLoop`.

No Tokio timer deadline is copied into the native loop. Tokio's worker owns its
timer driver and wakes the LocalSet when a sleep becomes ready. The reusable
Core Foundation timer represents only an application
`ControlFlow::WaitUntil` deadline. `ControlFlow::Poll` explicitly self-wakes;
`ControlFlow::Wait` sleeps until native input or a wake source fires.

One future can still perform arbitrarily long synchronous work in one poll.
This bridge provides wake integration, not preemption.

## Callback and reentrancy contract

`resumed`, `window_event`, `about_to_wait`, and `exiting` run with both the Tokio
runtime and persistent LocalSet context entered, so callbacks may use
`tokio::spawn`, `spawn_local`, timers, and I/O APIs.

The tick keeps a mutable `RefCell` borrow of the LocalSet while polling it. If a
native callback reenters during that poll, `with_external_context` observes the
failed borrow and runs under the already-installed runtime/local context rather
than recursively entering or polling the same LocalSet. Native events that
cannot borrow the application handler are deferred in FIFO order.

## macOS integration

The macOS backend installs:

- a level-0 `CFRunLoopSource` for cross-thread and LocalSet wakes;
- one reusable `CFRunLoopTimer` for application `WaitUntil` deadlines;
- both objects in `kCFRunLoopCommonModes` and
  `NSEventTrackingRunLoopMode` so work continues during live resize;
- a reentrancy guard and one deferred-retick flag;
- panic boundaries that prevent unwinding through Core Foundation callbacks.

When shutdown is requested, AppKit receives a synthetic application event after
`NSApplication::stop`; this makes the native run loop return normally instead
of requiring process termination.

## Shutdown order

The application requests native-loop exit only after its QuickJS lifecycle has
reached `Stopped` or `Failed`. `run_app_external` then:

1. invokes `ApplicationHandler::exiting` with runtime and LocalSet context;
2. unpublishes and invalidates native callbacks while retaining their Core
   Foundation objects;
3. drops local/QuickJS work;
4. drops and joins the Tokio runtime;
5. releases the retained Core Foundation objects and returns the application.

## LLRT / QuickJS example

`crates/winit/examples/cf_run_loop_tokio.rs` runs LLRT on the persistent
main-thread LocalSet. Its JavaScript increments a counter every second and
attempts a fetch every three seconds.

```sh
cargo run -p burokku-winit --example cf_run_loop_tokio
```

Manual macOS acceptance:

1. confirm the counter advances approximately once per second;
2. confirm fetch succeeds or reports the expected sandbox error every three
   seconds without stalling timers;
3. continuously resize the window for at least five seconds and observe both
   operations continue;
4. confirm the window remains responsive;
5. close the application and confirm normal QuickJS shutdown and process return.

The expected non-preemption demonstration is:

```sh
TOKIO_EXTERNAL_CPU_BLOCK=1 \
  cargo run -p burokku-winit --example cf_run_loop_tokio
```

The intentional two-second main-thread spin stalls the window and local timers.

## Automated coverage

`crates/runtime/tests/external_tokio.rs` proves against registry Tokio that:

- a one-worker runtime drives ordinary `tokio::spawn` away from the main thread;
- a proxy-style standard waker drives a manually polled persistent LocalSet;
- a local Tokio sleep resumes on the driving thread;
- LocalSet work beyond one internal poll slice requests another wake and drains;
- QuickJS evaluation, shutdown, and driver completion succeed.

`crates/winit/tests/external_wake_macos.rs` proves that a worker-side oneshot
wakes the main-thread LocalSet, `about_to_wait` requests exit, `exiting` runs,
and `run_app_external` returns before a watchdog.

Run:

```sh
cargo test -p runtime --locked
cargo test -p burokku-winit --locked
cargo check -p burokku-winit --example cf_run_loop_tokio --locked
cargo tree -i tokio --workspace
```

The dependency tree must contain exactly one registry Tokio version. A reliable
five-second AppKit drag remains a manual test.

## Platform scope

Only the macOS native backend is implemented. Other targets compile but return
`Error::UnsupportedPlatform`; this bridge does not add Windows, Linux, Wayland,
X11, or WASI backends.
