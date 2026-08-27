use std::time::Instant;

/// Wakes an event loop that owns the thread driving a current-thread runtime.
///
/// Implementations should only signal the platform event loop and return. They
/// must not call [`Runtime::tick_nonblocking`](super::Runtime::tick_nonblocking)
/// directly, since wake notifications may originate while Tokio is already in
/// a scheduler callback.
pub trait ExternalWake: Send + Sync + 'static {
    /// Signal the external event loop.
    fn wake(&self);
}

impl<F> ExternalWake for F
where
    F: Fn() + Send + Sync + 'static,
{
    fn wake(&self) {
        (self)()
    }
}

/// The outcome of one externally driven current-thread scheduler tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickResult {
    /// Tokio still has immediately runnable scheduler or local-set work.
    ///
    /// The platform integration should arrange another prompt tick when this
    /// is true rather than looping indefinitely in one native callback.
    pub has_more_work: bool,

    /// The earliest Tokio timer deadline currently known to the timer wheel.
    ///
    /// This is `None` when time is disabled or no timer is registered. The
    /// platform event loop should arm a native timer for this instant and call
    /// the runtime tick when it fires.
    pub next_deadline: Option<Instant>,

    /// Number of regular Tokio scheduler tasks polled by this tick.
    pub tasks_polled: usize,
}
