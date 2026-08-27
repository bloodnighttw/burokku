// Signal handling
cfg_signal_internal_and_unix! {
    mod signal;
}
cfg_io_uring! {
    mod uring;
    use uring::UringContext;
    use crate::sync::OnceCell;
}

use crate::io::interest::Interest;
use crate::io::ready::Ready;
use crate::loom::sync::Mutex;
use crate::runtime::driver;
use crate::runtime::io::registration_set;
use crate::runtime::park::{ParkThread, UnparkThread};
use crate::runtime::io::{IoDriverMetrics, RegistrationSet, ScheduledIo};

use mio::event::Source;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// I/O driver, backed by Mio.
pub(crate) struct Driver {
    mode: DriverMode,

    /// True when an event with the signal token is received.
    signal_ready: Arc<AtomicBool>,
}

enum DriverMode {
    Inline(Reactor),

    #[cfg(not(target_os = "wasi"))]
    Threaded(ThreadedReactor),
}

struct Reactor {
    /// Reuse the `mio::Events` value across calls to poll.
    events: mio::Events,

    /// The system event queue. Exactly one thread owns and polls this value.
    poll: mio::Poll,

    signal_ready: Arc<AtomicBool>,
}

#[cfg(not(target_os = "wasi"))]
struct ThreadedReactor {
    stop: Arc<AtomicBool>,
    poll_waker: Arc<mio::Waker>,
    parker: ParkThread,
    thread: Option<JoinHandle<()>>,
}

struct ReactorHandle {
    registrations: Arc<RegistrationSet>,
    synced: Arc<Mutex<registration_set::Synced>>,
    metrics: Arc<IoDriverMetrics>,

    #[cfg(all(
        tokio_unstable,
        feature = "io-uring",
        feature = "rt",
        feature = "fs",
        target_os = "linux",
    ))]
    uring_context: Arc<Mutex<UringContext>>,
}

/// A reference to an I/O driver.
pub(crate) struct Handle {
    /// Registers I/O resources.
    registry: mio::Registry,

    /// Tracks all registrations
    registrations: Arc<RegistrationSet>,

    /// State that should be synchronized
    synced: Arc<Mutex<registration_set::Synced>>,

    /// Used to wake up the reactor from a call to `turn`.
    /// Not supported on `Wasi` due to lack of threading support.
    #[cfg(not(target_os = "wasi"))]
    waker: Arc<mio::Waker>,

    /// Wakes a scheduler thread parked outside Mio when the reactor is split.
    scheduler_unpark: Option<UnparkThread>,

    pub(crate) metrics: Arc<IoDriverMetrics>,

    #[cfg(all(
        tokio_unstable,
        feature = "io-uring",
        feature = "rt",
        feature = "fs",
        target_os = "linux",
    ))]
    pub(crate) uring_context: Arc<Mutex<UringContext>>,

    #[cfg(all(
        tokio_unstable,
        feature = "io-uring",
        feature = "rt",
        feature = "fs",
        target_os = "linux",
    ))]
    pub(crate) uring_probe: OnceCell<Option<io_uring::Probe>>,
}

#[derive(Debug)]
pub(crate) struct ReadyEvent {
    pub(super) tick: u8,
    pub(crate) ready: Ready,
    pub(super) is_shutdown: bool,
}

cfg_net_unix!(
    impl ReadyEvent {
        pub(crate) fn with_ready(&self, ready: Ready) -> Self {
            Self {
                ready,
                tick: self.tick,
                is_shutdown: self.is_shutdown,
            }
        }
    }
);

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub(super) enum Direction {
    Read,
    Write,
}

pub(super) enum Tick {
    Set,
    Clear(u8),
}

const TOKEN_WAKEUP: mio::Token = mio::Token(0);
const TOKEN_SIGNAL: mio::Token = mio::Token(1);

fn _assert_kinds() {
    fn _assert<T: Send + Sync>() {}

    _assert::<Handle>();
}

// ===== impl Driver =====

impl Driver {
    /// Creates a new event loop, returning any error that happened during the
    /// creation.
    pub(crate) fn new(
        nevents: usize,
        external_wake: Option<Arc<dyn crate::runtime::ExternalWake>>,
    ) -> io::Result<(Driver, Handle)> {
        let poll = mio::Poll::new()?;
        #[cfg(not(target_os = "wasi"))]
        let waker = Arc::new(mio::Waker::new(poll.registry(), TOKEN_WAKEUP)?);
        let registry = poll.registry().try_clone()?;
        let signal_ready = Arc::new(AtomicBool::new(false));
        let reactor = Reactor {
            signal_ready: signal_ready.clone(),
            events: mio::Events::with_capacity(nevents),
            poll,
        };

        let (registrations, synced) = RegistrationSet::new();
        let registrations = Arc::new(registrations);
        let synced = Arc::new(Mutex::new(synced));
        let metrics = Arc::new(IoDriverMetrics::default());

        #[cfg(all(
            tokio_unstable,
            feature = "io-uring",
            feature = "rt",
            feature = "fs",
            target_os = "linux",
        ))]
        let uring_context = Arc::new(Mutex::new(UringContext::new()));

        let reactor_handle = ReactorHandle {
            registrations: registrations.clone(),
            synced: synced.clone(),
            metrics: metrics.clone(),
            #[cfg(all(
                tokio_unstable,
                feature = "io-uring",
                feature = "rt",
                feature = "fs",
                target_os = "linux",
            ))]
            uring_context: uring_context.clone(),
        };

        #[cfg(not(target_os = "wasi"))]
        let (mode, scheduler_unpark) = if let Some(external_wake) = external_wake {
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = stop.clone();
            let poll_waker = waker.clone();
            let parker = ParkThread::new();
            let scheduler_unpark = parker.unpark();
            let reactor_scheduler_unpark = scheduler_unpark.clone();
            let thread = std::thread::Builder::new()
                .name("tokio-mio-reactor".to_owned())
                .spawn(move || {
                    let mut reactor = reactor;
                    loop {
                        reactor.turn(&reactor_handle, None);
                        // Special signal/process tokens do not wake a
                        // ScheduledIo, so wake an ordinary block_on parker
                        // directly after every completed Mio turn.
                        reactor_scheduler_unpark.unpark();
                        if thread_stop.load(Ordering::Acquire) {
                            break;
                        }
                        external_wake.wake();
                    }
                })?;
            (
                DriverMode::Threaded(ThreadedReactor {
                    stop,
                    poll_waker,
                    parker,
                    thread: Some(thread),
                }),
                Some(scheduler_unpark),
            )
        } else {
            (DriverMode::Inline(reactor), None)
        };

        #[cfg(target_os = "wasi")]
        let (mode, scheduler_unpark) = {
            let _ = external_wake;
            (DriverMode::Inline(reactor), None)
        };

        let handle = Handle {
            registry,
            registrations,
            synced,
            #[cfg(not(target_os = "wasi"))]
            waker,
            scheduler_unpark,
            metrics,
            #[cfg(all(
                tokio_unstable,
                feature = "io-uring",
                feature = "rt",
                feature = "fs",
                target_os = "linux",
            ))]
            uring_context,
            #[cfg(all(
                tokio_unstable,
                feature = "io-uring",
                feature = "rt",
                feature = "fs",
                target_os = "linux",
            ))]
            uring_probe: OnceCell::new(),
        };

        Ok((
            Driver {
                mode,
                signal_ready,
            },
            handle,
        ))
    }

    pub(crate) fn park(&mut self, rt_handle: &driver::Handle) {
        match &mut self.mode {
            DriverMode::Inline(reactor) => reactor.turn(&ReactorHandle::from(rt_handle.io()), None),
            #[cfg(not(target_os = "wasi"))]
            DriverMode::Threaded(threaded) => threaded.parker.park(),
        }
    }

    pub(crate) fn park_timeout(&mut self, rt_handle: &driver::Handle, duration: Duration) {
        match &mut self.mode {
            DriverMode::Inline(reactor) => {
                reactor.turn(&ReactorHandle::from(rt_handle.io()), Some(duration))
            }
            #[cfg(not(target_os = "wasi"))]
            DriverMode::Threaded(threaded) => threaded.parker.park_timeout(duration),
        }
    }

    pub(crate) fn shutdown(&mut self, rt_handle: &driver::Handle) {
        let handle = rt_handle.io();

        #[cfg(not(target_os = "wasi"))]
        if let DriverMode::Threaded(threaded) = &mut self.mode {
            threaded.stop_and_join();
        }

        let ios = handle.registrations.shutdown(&mut handle.synced.lock());

        // `shutdown()` must be called without holding the lock.
        for io in ios {
            io.shutdown();
        }
    }
}

impl Reactor {
    fn turn(&mut self, handle: &ReactorHandle, max_wait: Option<Duration>) {
        debug_assert!(!handle.registrations.is_shutdown(&handle.synced.lock()));

        handle.release_pending_registrations();

        let events = &mut self.events;
        match self.poll.poll(events, max_wait) {
            Ok(()) => {}
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            #[cfg(target_os = "wasi")]
            Err(e) if e.kind() == io::ErrorKind::InvalidInput => {}
            Err(e) => panic!("unexpected error when polling the I/O driver: {e:?}"),
        }

        let mut ready_count = 0;
        for event in events.iter() {
            let token = event.token();

            if token == TOKEN_WAKEUP {
                // Control notification only.
            } else if token == TOKEN_SIGNAL {
                self.signal_ready.store(true, Ordering::Release);
            } else {
                let ready = Ready::from_mio(event);
                let ptr = super::EXPOSE_IO.from_exposed_addr(token.0);

                // Safety: the registration set retains an Arc until Mio has
                // deregistered the source and this sole poll owner has crossed
                // a subsequent turn boundary.
                let io: &ScheduledIo = unsafe { &*ptr };
                io.set_readiness(Tick::Set, |curr| curr | ready);
                io.wake(ready);
                ready_count += 1;
            }
        }

        #[cfg(all(
            tokio_unstable,
            feature = "io-uring",
            feature = "rt",
            feature = "fs",
            target_os = "linux",
        ))]
        {
            let mut guard = handle.uring_context.lock();
            let ctx = &mut *guard;
            ctx.dispatch_completions();
            while ctx
                .uring
                .as_mut()
                .is_some_and(|uring| uring.submission().cq_overflow())
            {
                ctx.submit()
                    .expect("failed to flush io_uring completion queue overflow");
                ctx.dispatch_completions();
            }
        }

        handle.metrics.incr_ready_count_by(ready_count);
    }
}

impl ReactorHandle {
    fn release_pending_registrations(&self) {
        if self.registrations.needs_release() {
            self.registrations.release(&mut self.synced.lock());
        }
    }
}

impl From<&Handle> for ReactorHandle {
    fn from(handle: &Handle) -> Self {
        Self {
            registrations: handle.registrations.clone(),
            synced: handle.synced.clone(),
            metrics: handle.metrics.clone(),
            #[cfg(all(
                tokio_unstable,
                feature = "io-uring",
                feature = "rt",
                feature = "fs",
                target_os = "linux",
            ))]
            uring_context: handle.uring_context.clone(),
        }
    }
}

#[cfg(not(target_os = "wasi"))]
impl ThreadedReactor {
    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.poll_waker.wake();
        if let Some(thread) = self.thread.take() {
            thread.join().expect("Tokio Mio reactor thread panicked");
        }
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        #[cfg(not(target_os = "wasi"))]
        if let DriverMode::Threaded(threaded) = &mut self.mode {
            if threaded.thread.is_some() {
                threaded.stop.store(true, Ordering::Release);
                let _ = threaded.poll_waker.wake();
                if let Some(thread) = threaded.thread.take() {
                    let _ = thread.join();
                }
            }
        }
    }
}

impl fmt::Debug for Driver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Driver")
    }
}

impl Handle {
    /// Forces a reactor blocked in a call to `turn` to wakeup, or otherwise
    /// makes the next call to `turn` return immediately.
    ///
    /// This method is intended to be used in situations where a notification
    /// needs to otherwise be sent to the main reactor. If the reactor is
    /// currently blocked inside of `turn` then it will wake up and soon return
    /// after this method has been called. If the reactor is not currently
    /// blocked in `turn`, then the next call to `turn` will not block and
    /// return immediately.
    pub(crate) fn unpark(&self) {
        #[cfg(not(target_os = "wasi"))]
        self.waker.wake().expect("failed to wake I/O driver");

        if let Some(unpark) = &self.scheduler_unpark {
            unpark.unpark();
        }
    }

    /// Registers an I/O resource with the reactor for a given `mio::Ready` state.
    ///
    /// The registration token is returned.
    pub(super) fn add_source(
        &self,
        source: &mut impl mio::event::Source,
        interest: Interest,
    ) -> io::Result<Arc<ScheduledIo>> {
        let scheduled_io = self.registrations.allocate(&mut self.synced.lock())?;
        let token = scheduled_io.token();

        // we should remove the `scheduled_io` from the `registrations` set if registering
        // the `source` with the OS fails. Otherwise it will leak the `scheduled_io`.
        if let Err(e) = self.registry.register(source, token, interest.to_mio()) {
            // safety: `scheduled_io` is part of the `registrations` set.
            unsafe {
                self.registrations
                    .remove(&mut self.synced.lock(), &scheduled_io)
            };

            return Err(e);
        }

        // TODO: move this logic to `RegistrationSet` and use a `CountedLinkedList`
        self.metrics.incr_fd_count();

        Ok(scheduled_io)
    }

    /// Deregisters an I/O resource from the reactor.
    pub(super) fn deregister_source(
        &self,
        registration: &Arc<ScheduledIo>,
        source: &mut impl Source,
    ) -> io::Result<()> {
        // Deregister the source with the OS poller **first**
        // Cleanup ALWAYS happens
        let os_result = self.registry.deregister(source);

        let reached_release_batch = self
            .registrations
            .deregister(&mut self.synced.lock(), registration);
        if reached_release_batch || self.scheduler_unpark.is_some() {
            // A dedicated reactor must cross a poll boundary before the token's
            // retained Arc can be released, even when fewer than the inline
            // driver's batching threshold have accumulated.
            self.unpark();
        }

        self.metrics.dec_fd_count();

        os_result // Return error after cleanup
    }

    fn release_pending_registrations(&self) {
        if self.registrations.needs_release() {
            self.registrations.release(&mut self.synced.lock());
        }
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle")
    }
}

impl Direction {
    pub(super) fn mask(self) -> Ready {
        match self {
            Direction::Read => Ready::READABLE | Ready::READ_CLOSED,
            Direction::Write => Ready::WRITABLE | Ready::WRITE_CLOSED,
        }
    }
}
