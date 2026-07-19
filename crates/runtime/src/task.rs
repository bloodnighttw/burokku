//! The two kinds of work processed by the JavaScript event loop.
//!
//! QuickJS owns the microtask queue used by promises.  The async runtime drives
//! that queue after each poll of a host task.  Keeping the host task kinds
//! explicit here makes the ordering contract visible at the call sites:
//! timers are macrotasks, while promise continuations are microtasks.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A host task which runs as a macrotask.
pub(crate) struct Macrotask<F> {
    future: F,
}

impl<F> Macrotask<F> {
    pub(crate) fn new(future: F) -> Self {
        Self { future }
    }
}

impl<F> Future for Macrotask<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // `Macrotask` does not move its future after it has been pinned.
        unsafe { self.map_unchecked_mut(|task| &mut task.future) }.poll(context)
    }
}

/// A host task which runs as a microtask.
///
/// Promise reactions are queued by QuickJS itself.  This wrapper is used when
/// Rust awaits a promise so that promise work follows the same explicit task
/// model as host work; the QuickJS driver remains responsible for draining the
/// actual promise job queue.
pub(crate) struct Microtask<F> {
    future: F,
}

impl<F> Microtask<F> {
    pub(crate) fn new(future: F) -> Self {
        Self { future }
    }
}

impl<F> Future for Microtask<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        // `Microtask` does not move its future after it has been pinned.
        unsafe { self.map_unchecked_mut(|task| &mut task.future) }.poll(context)
    }
}
