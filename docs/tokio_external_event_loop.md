# Platform-driven Tokio current-thread prototype

This repository imports the full-history Tokio 1.53.1 repository as a
non-squashed Git subtree in `crates/tokio`. The patched Tokio crate lives at
`crates/tokio/tokio` and is selected through the workspace's
`[patch.crates-io]` entry. The subtree is excluded as a parent-workspace member;
run Tokio's own tests through its nested workspace manifest.

The patch is a proof of concept. It preserves Tokio networking, Mio, the normal
readiness types, the timer wheel, and current-thread task scheduling. It changes
who owns the outer wait:

```text
platform main loop                 tokio-mio-reactor
------------------                 -----------------
Runtime::tick_nonblocking()        mio::Poll::poll(..., None)
Tokio scheduler tasks              ScheduledIo::set_readiness
LocalSet / !Send futures           ScheduledIo::wake
Tokio timer-wheel expiration       ExternalWake::wake
UI / QuickJS / LLRT                (never polls application tasks)
```

## Why upstream `current_thread` cannot be embedded this way

The upstream scheduler stores its entire `Core` in an `AtomicCell`. A
`Runtime::block_on` caller steals that core and enters an unbounded loop. When
no task is ready, `Context::park` moves the composite runtime driver out of the
core and blocks in it. The traditional time driver computes its next wheel
expiration and delegates the wait through signal/process layers to the Tokio
I/O driver, whose `Driver::turn` calls `mio::Poll::poll`.

Consequently, scheduler execution, timer expiration, readiness polling, and the
outer blocking wait are coupled to the thread running `block_on`. Tokio has a
useful internal `park_yield` operation, but no public operation that acquires the
core, performs a bounded number of scheduler polls, and returns a deadline.
Repeated `block_on(yield_now())` would still use root-future machinery, would
not provide a reliable work/deadline contract, and would duplicate scheduler
policy in the embedding layer.

Important upstream seams used by this patch:

- `runtime/scheduler/current_thread/mod.rs`: `take_core`, `CoreGuard::enter`,
  `Core::next_task`, `Context::run_task`, and `park_yield`;
- `runtime/driver.rs`: composite I/O/signal/process/time driver and unpark fanout;
- `runtime/io/driver.rs`: exclusive `Poll`/`Events` owner and shared cloned
  `Registry` handle;
- `runtime/time/mod.rs`: traditional timer wheel processing and earliest
  expiration;
- `runtime/io/scheduled_io.rs`: atomic readiness publication and waiter waking;
- `runtime/io/registration_set.rs`: deferred `ScheduledIo` reclamation.

## Public prototype API

```rust
use std::sync::Arc;
use tokio::runtime::{Builder, ExternalWake, TickResult};

struct PlatformWake;
impl ExternalWake for PlatformWake {
    fn wake(&self) {
        // PostMessage, eventfd write, CFRunLoopSourceSignal, ...
    }
}

let runtime = Builder::new_current_thread()
    .enable_all()
    .external_event_loop(Arc::new(PlatformWake))
    .external_tick_budget(64)
    .build()?;

let TickResult {
    has_more_work,
    next_deadline,
    tasks_polled,
} = runtime.tick_nonblocking();
```

`Runtime::tick_nonblocking_with_local_set(&mut LocalSet)` polls a `LocalSet` in
the same scheduler entry, and `LocalRuntime::tick_nonblocking` supports the
thread-affine local runtime. The regular `Runtime`, `Handle`, `tokio::spawn`,
`LocalSet::spawn_local`, `tokio::net`, `tokio::time`, `tokio::sync`, and
`spawn_blocking` APIs remain intact.

A tick:

1. acquires the current-thread `Core` without waiting;
2. enters Tokio's scheduler/runtime TLS;
3. performs a zero-duration composite-driver turn, expiring elapsed timers;
4. optionally polls the supplied `LocalSet` root;
5. polls at most `external_tick_budget` regular scheduler tasks;
6. performs another zero-duration driver turn so timers registered by those
   tasks are visible;
7. signals `ExternalWake` when the budget leaves immediately runnable work;
8. returns immediate-work state and the earliest timer-wheel deadline.

A returned deadline only covers timers registered by tasks polled so far. When
`has_more_work` is true, the automatic wake requests another bounded platform
callback instead of allowing the host to sleep until that partial deadline.
This promptly exposes an earlier timer created by a task beyond the previous
budget while preserving UI responsiveness.

The count bound is not preemption. One future can perform arbitrarily long
synchronous work in one `poll`, which blocks both AppKit and Tokio as expected.
Timer expiration can also wake a large elapsed batch in one turn, and a
`LocalSet` has its own bounded queue in addition to the regular task budget.
Recursive or concurrent ticks panic rather than waiting for the scheduler core.
Paused/virtual Tokio time is not supported by native deadline driving; do not
call `start_paused` or `tokio::time::pause` in this mode.

An integration using `LocalSet` should retain and consistently pass one
persistent set to `tick_nonblocking_with_local_set`. Do not alternate plain
runtime ticks or multiple sets, because Tokio exposes one root-wake bit.

## Mio reactor split

Mio already has the required ownership split: `Poll::poll` requires exclusive
mutable access, while a cloned `Registry` supports registration from other
threads. In external mode, `runtime/io/driver.rs` moves the existing poll and
event buffer into one thread named `tokio-mio-reactor`. No mutex permits a
second poll caller.

The reactor runs the existing readiness dispatch:

```text
Mio Event
  -> Ready::from_mio
  -> ScheduledIo::set_readiness(Tick::Set, ...)
  -> ScheduledIo::wake
  -> current-thread remote injection queue
  -> ExternalWake::wake
```

The main-thread composite driver's I/O park becomes a Tokio `ParkThread`. A
zero-duration tick therefore never touches `mio::Poll`; ordinary `block_on`
remains capable of parking if an embedding uses it outside the platform loop.
The reactor unparks that scheduler parker after every Mio turn, including
special Unix signal/process tokens that do not wake a `ScheduledIo`.
The Mio waker still handles source registration/deregistration pressure and
reactor shutdown.

On runtime shutdown, the patch sets the reactor stop flag, wakes Mio, joins the
reactor, marks the registration set shut down, and only then shuts down retained
`ScheduledIo` values.

## Wake and timer contract

The external callback is attached below the timer driver at `IoHandle::unpark`,
not only at the scheduler handle. This covers:

- tasks spawned or woken from another thread;
- Tokio synchronization that makes a task runnable;
- readiness published by the Mio reactor;
- insertion/reset of a timer earlier than the timer driver's cached wake;
- ordinary scheduler unpark transitions.

The callback can execute on the main thread, the Mio reactor, or another
producer thread. It is also invoked before a bounded tick returns when
`has_more_work` is true, so work left by `external_tick_budget` causes a prompt
follow-up callback rather than a wait for `next_deadline`. It must only signal
the native loop and return. It must not recursively tick Tokio. Platform
integrations should coalesce signals if their native primitive does not already
do so.

`next_deadline` queries the authoritative traditional timer wheel under its
existing lock and converts the remaining duration using Tokio's runtime clock.
If a producer inserts an earlier timer after a tick returns, that insertion also
calls `ExternalWake`, causing the platform loop to tick and re-arm.

## AppKit proof of concept

`crates/winit/examples/cf_run_loop_tokio.rs` creates an `NSApplication`, an
`NSWindow`, a level-0 `CFRunLoopSource`, and a reusable `CFRunLoopTimer`. It
installs selected LLRT 0.9 modules into the repository's rquickjs runtime and
runs `crates/winit/examples/cf_run_loop_tokio.js` on a persistent `LocalSet`.
The script increments a counter every second and fetches `https://example.com`
every three seconds. LLRT is pinned to a Git revision and shares rquickjs 0.12
and this workspace's patched Tokio through Cargo dependency resolution.

Both source and timer are installed in:

- `kCFRunLoopCommonModes`; and
- `NSEventTrackingRunLoopMode` explicitly.

The explicit tracking-mode registration is what keeps Tokio, rquickjs, and LLRT
progressing during AppKit's nested live-resize loop.

Build and run:

```sh
cargo run -p burokku-winit --example cf_run_loop_tokio
```

Manual acceptance procedure:

1. Confirm `[llrt] count = N` prints approximately once per second.
2. Confirm `[llrt] fetched example.com: status=200, ...` prints approximately
   once per three seconds. A sandbox without direct network access may instead
   print the script's `[llrt] fetch failed: ...` diagnostic.
3. Grab a window resize handle and continuously resize for at least five
   seconds. Observe the counter and at least one fetch attempt while the drag is
   still active.
4. Confirm the window remains responsive and counter output has no multi-second
   gap ending only when resize stops.
5. Confirm the Rust main-thread assertions do not fail.

Expected limitation test:

```sh
TOKIO_EXTERNAL_CPU_BLOCK=1 \
  cargo run -p burokku-winit --example cf_run_loop_tokio
```

After three seconds, one Tokio task spins synchronously for two seconds. The
window and timer intentionally stall for those two seconds, demonstrating that
the patch moves only blocking readiness waiting—not application computation.

The source callback has a reentrancy guard because AppKit can nest run loops.
A nested notification is deferred by re-signalling the source. During teardown,
the runtime and its producers are stopped and the Mio reactor is joined while
the CF source is still retained; only then are source/timer callbacks invalidated
and released.

## Automated coverage

`crates/tokio/tokio/tests/rt_external_driver.rs` covers:

- task-poll budget enforcement and `has_more_work`;
- timer deadline propagation;
- TCP progress where loopback sockets are permitted;
- readiness publication through a Unix socket pair, including a required
  reactor-thread `ExternalWake` callback (works in hermetic macOS runners that
  deny TCP bind);
- application task thread identity before and after I/O/timer suspension;
- timer deadline accuracy and expiration independent of TCP availability;
- blocking-pool completion wake propagation;
- `Rc<Cell<_>>` work through `LocalSet` on the driving thread;
- bounded reactor join with an outstanding registration;
- the repository's actual QuickJS `RuntimeDriver`, eval/macrotask path, and
  shutdown on the external main-thread tick (`crates/runtime/tests/external_tokio.rs`).

Run:

```sh
cargo test --manifest-path crates/tokio/Cargo.toml -p tokio \
  --test rt_external_driver --features full
cargo test -p runtime --test external_tokio
cargo check -p burokku-winit --example cf_run_loop_tokio
```

An automated test cannot synthesize a trustworthy five-second AppKit drag in a
headless unit-test runner; the example is the behavioral test harness for that
manual interaction.

## LLRT / QuickJS compatibility

The patch does not introduce replacement networking, timer, channel, or spawn
APIs. LLRT code can continue using `tokio::net`, `tokio::time`, `tokio::sync`,
and `tokio::spawn`. Keep the runtime and the LLRT/QuickJS owner in the platform
main-loop state and invoke ticks only from that thread. Use `LocalSet` (via
`tick_nonblocking_with_local_set`) or `LocalRuntime` for Rust futures holding
`Rc`, `RefCell`, rquickjs values, or UI objects.

The reactor receives only Tokio's Send/Sync readiness bookkeeping. It has no
scheduler core and no path that polls LLRT or application futures.
`crates/runtime/tests/external_tokio.rs` exercises the repository's real
rquickjs runtime driver, JavaScript evaluation, macrotask wakeups, and shutdown
through a persistent main-thread `LocalSet`.

## Windows and Linux integration

The Tokio API is platform-neutral and Mio remains the backend.

- **Win32:** implement `ExternalWake` with `PostMessageW`, a manual-reset event,
  or another message-loop wake. Translate `next_deadline` to a waitable timer or
  message-loop timer. Mio's IOCP-backed `Poll` stays exclusively on the reactor
  thread.
- **X11:** signal an `eventfd` or pipe watched beside the X connection; arm
  `timerfd` or the host loop's timer from `next_deadline`.
- **Wayland:** signal an `eventfd`/pipe integrated with the display poll and use
  the compositor loop's timer facility. Do not call a Tokio tick while holding
  Wayland dispatch locks if callbacks can reenter application code.

WASI lacks the required threading/waker support. Building external mode with
I/O enabled therefore returns `Unsupported` instead of silently falling back to
an idle inline reactor.

## Internal invariants

The patch relies on all of the following:

1. Exactly one thread owns `mio::Poll`, mutable `mio::Events`, and calls `poll`.
2. Registration uses only a cloned `Registry` for that same poll instance.
3. OS deregistration happens before Tokio registration bookkeeping removal.
4. `RegistrationSet` retains each token's `Arc<ScheduledIo>` until the reactor
   crosses the required post-deregistration poll boundary.
5. Readiness is atomically published before waiter wakers run.
6. `ScheduledIo` invokes wakers outside waiter/registration locks.
7. Existing generation-qualified readiness clearing remains unchanged.
8. The current-thread core has only one driver at a time. Explicit
   `UnhandledPanic::ShutdownRuntime` detection returns the core from
   `CoreGuard::enter` before propagating its public panic.
9. Application futures are polled only through `Context::run_task` (or the
   LocalSet root) while current-thread scheduler TLS is installed.
10. Timer-wheel mutation and queries use the existing timer lock.
11. External wake callbacks are nonblocking and nonreentrant.
12. Runtime shutdown joins the reactor before freeing registration tokens.

## Maintenance and rebase assessment

This is a moderate-to-high maintenance internal patch. The public surface is
small, but the implementation touches Tokio's most change-sensitive internals:
current-thread scheduling, composite driver construction, I/O token lifetime,
and timer-wheel inspection.

Likely easy rebase areas:

- `ExternalWake`, `TickResult`, builder plumbing, and platform example;
- the bounded loop if `CoreGuard`, `Context::run_task`, and `park_yield` retain
  their current roles.

Likely conflict areas:

- Tokio driver-stack refactors;
- changes to signal/process wrapping;
- Mio registration reclamation or token provenance;
- alternative timer adoption by current-thread runtimes;
- scheduler fairness/metrics loop changes;
- io-uring integration.

For each Tokio upgrade, diff the upstream versions of the five investigation
areas listed above; re-check panic/unwind paths, signal/process parker handoff,
AppKit wake-source lifetime, standalone manifest test registration, and the
Windows/Linux/WASI feature matrix; rerun the focused external-driver tests and
Tokio's I/O/time/current-thread suites; then repeat the manual AppKit live-resize
test. Keeping the patch as an opt-in builder mode and
reusing the canonical readiness/task paths limits—but does not eliminate—the
rebase cost.
