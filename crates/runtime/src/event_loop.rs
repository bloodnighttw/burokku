//! The runtime-owned JavaScript event loop.
//!
//! Host integrations enqueue macrotasks here. After every macrotask, the
//! runtime drains QuickJS's native job queue, which is where promise reactions
//! and the rest of JavaScript's microtasks live.

use crate::{Plugin, Result};
use rquickjs::{AsyncContext, Ctx, JsLifetime};
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

/// Default number of JavaScript macrotasks that may wait for an isolate.
pub const DEFAULT_MACROTASK_CAPACITY: usize = 1024;

trait Macrotask: Send {
    fn run(self: Box<Self>, context: &Ctx<'_>) -> Result<()>;
}

impl<F> Macrotask for F
where
    F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
{
    fn run(self: Box<Self>, context: &Ctx<'_>) -> Result<()> {
        (*self)(context)
    }
}

type MacrotaskMessage = Box<dyn Macrotask>;

struct ShutdownRequest {
    acknowledge: Option<oneshot::Sender<()>>,
}

/// Failure to submit a macrotask to an isolate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacrotaskQueueError {
    /// The bounded queue has no remaining capacity.
    Full,
    /// The runtime has stopped accepting tasks.
    Closed,
}

impl std::fmt::Display for MacrotaskQueueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => formatter.write_str("the JavaScript macrotask queue is full"),
            Self::Closed => formatter.write_str("the JavaScript macrotask queue is closed"),
        }
    }
}

impl std::error::Error for MacrotaskQueueError {}

#[derive(Debug)]
pub(crate) struct RuntimeControl {
    shutdown: Sender<ShutdownRequest>,
}

impl RuntimeControl {
    pub(crate) fn request_shutdown(
        &self,
    ) -> std::result::Result<oneshot::Receiver<()>, MacrotaskQueueError> {
        let (sender, receiver) = oneshot::channel();
        self.shutdown
            .try_send(ShutdownRequest {
                acknowledge: Some(sender),
            })
            .map_err(map_try_send_error)?;
        Ok(receiver)
    }

    pub(crate) fn request_shutdown_without_waiting(&self) {
        let _ = self
            .shutdown
            .try_send(ShutdownRequest { acknowledge: None });
    }
}

/// A cloneable handle to the runtime's macrotask queue.
///
/// Plugins can retrieve the handle during installation and move it into host
/// callbacks or async work. A queued callback is always invoked with the
/// runtime's QuickJS context, followed by a native QuickJS microtask
/// checkpoint.
#[derive(Clone, Debug)]
pub struct MacrotaskQueue {
    sender: Sender<MacrotaskMessage>,
}

// The queue contains no JavaScript values and does not depend on `'js`.
unsafe impl<'js> JsLifetime<'js> for MacrotaskQueue {
    type Changed<'to> = MacrotaskQueue;
}

impl MacrotaskQueue {
    /// Retrieve the queue installed in a runtime context.
    pub fn from_context(context: &Ctx<'_>) -> Result<Self> {
        context
            .userdata::<Self>()
            .map(|queue| queue.clone())
            .ok_or(rquickjs::Error::Unknown)
    }

    /// Enqueue one JavaScript macrotask, waiting for bounded capacity.
    ///
    /// Use this from asynchronous producers. Synchronous JavaScript host
    /// callbacks should use [`Self::try_enqueue`] so the isolate cannot
    /// deadlock waiting for itself to consume the queue.
    pub async fn enqueue<F>(&self, task: F) -> std::result::Result<(), MacrotaskQueueError>
    where
        F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
    {
        self.sender
            .send(Box::new(task))
            .await
            .map_err(|_| MacrotaskQueueError::Closed)
    }

    /// Attempt to enqueue from a synchronous callback without waiting.
    pub fn try_enqueue<F>(&self, task: F) -> std::result::Result<(), MacrotaskQueueError>
    where
        F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
    {
        self.sender
            .try_send(Box::new(task))
            .map_err(map_try_send_error)
    }

    /// Remaining task slots currently available to producers.
    pub fn capacity(&self) -> usize {
        self.sender.capacity()
    }

    /// Configured bounded capacity of this queue.
    pub fn max_capacity(&self) -> usize {
        self.sender.max_capacity()
    }

    /// Number of macrotasks currently waiting to run.
    pub fn depth(&self) -> usize {
        self.max_capacity().saturating_sub(self.capacity())
    }
}

fn map_try_send_error<T>(error: mpsc::error::TrySendError<T>) -> MacrotaskQueueError {
    match error {
        mpsc::error::TrySendError::Full(_) => MacrotaskQueueError::Full,
        mpsc::error::TrySendError::Closed(_) => MacrotaskQueueError::Closed,
    }
}

pub(crate) async fn install(
    context: &AsyncContext,
    capacity: usize,
    plugins: Vec<Box<dyn Plugin>>,
) -> Result<(MacrotaskQueue, RuntimeControl, oneshot::Receiver<()>)> {
    let (sender, receiver) = mpsc::channel(capacity);
    let (shutdown_sender, shutdown_receiver) = mpsc::channel(1);
    let (stopped_sender, stopped_receiver) = oneshot::channel();
    let queue = MacrotaskQueue { sender };
    let control = RuntimeControl {
        shutdown: shutdown_sender,
    };

    context
        .with({
            let queue = queue.clone();
            move |context| -> Result<()> {
                context
                    .store_userdata(queue)
                    .map_err(|_| rquickjs::Error::Unknown)?;
                for plugin in &plugins {
                    plugin.install(&context)?;
                }
                context.spawn(run(
                    context.clone(),
                    receiver,
                    shutdown_receiver,
                    stopped_sender,
                ));
                Ok(())
            }
        })
        .await?;

    Ok((queue, control, stopped_receiver))
}

async fn run<'js>(
    context: Ctx<'js>,
    mut tasks: Receiver<MacrotaskMessage>,
    mut control: Receiver<ShutdownRequest>,
    stopped: oneshot::Sender<()>,
) {
    loop {
        let task = tokio::select! {
            biased;
            request = control.recv() => {
                if let Some(ShutdownRequest {
                    acknowledge: Some(acknowledge),
                }) = request
                {
                    let _ = acknowledge.send(());
                }
                break;
            }
            task = tasks.recv() => {
                let Some(task) = task else {
                    break;
                };
                task
            }
        };

        if let Err(error) = task.run(&context) {
            eprintln!("JavaScript macrotask failed: {error}");
        }

        // Promises use QuickJS's own job queue. Draining it here establishes
        // the JavaScript rule that all ready microtasks run before the next
        // macrotask.
        while context.execute_pending_job() {}
    }

    let _ = stopped.send(());
}
