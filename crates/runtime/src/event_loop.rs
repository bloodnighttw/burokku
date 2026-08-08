//! The runtime-owned JavaScript event loop.
//!
//! Host integrations enqueue macrotasks here. After every macrotask, the
//! runtime drains QuickJS's native job queue, which is where promise reactions
//! and the rest of JavaScript's microtasks live.

use crate::Result;
use rquickjs::{AsyncContext, Ctx, JsLifetime};
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    oneshot,
};

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

enum MacrotaskMessage {
    Run(Box<dyn Macrotask>),
    Shutdown(Option<oneshot::Sender<()>>),
}

/// A cloneable handle to the runtime's macrotask queue.
///
/// Plugins can retrieve the handle during installation and move it into host
/// callbacks or async work. A queued callback is always invoked with the
/// runtime's QuickJS context, followed by a native QuickJS microtask
/// checkpoint.
#[derive(Clone, Debug)]
pub struct MacrotaskQueue {
    sender: UnboundedSender<MacrotaskMessage>,
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

    /// Enqueue one JavaScript macrotask.
    pub fn enqueue<F>(&self, task: F) -> Result<()>
    where
        F: for<'js> FnOnce(&Ctx<'js>) -> Result<()> + Send + 'static,
    {
        self.sender
            .send(MacrotaskMessage::Run(Box::new(task)))
            .map_err(|_| rquickjs::Error::Unknown)
    }

    pub(crate) fn request_shutdown(&self) -> Result<oneshot::Receiver<()>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(MacrotaskMessage::Shutdown(Some(sender)))
            .map_err(|_| rquickjs::Error::Unknown)?;
        Ok(receiver)
    }

    pub(crate) fn request_shutdown_without_waiting(&self) {
        let _ = self.sender.send(MacrotaskMessage::Shutdown(None));
    }
}

pub(crate) async fn install(
    context: &AsyncContext,
) -> Result<(MacrotaskQueue, oneshot::Receiver<()>)> {
    let (sender, receiver) = mpsc::unbounded_channel();
    let (stopped_sender, stopped_receiver) = oneshot::channel();
    let queue = MacrotaskQueue { sender };

    context
        .with({
            let queue = queue.clone();
            move |context| -> Result<()> {
                context
                    .store_userdata(queue)
                    .map_err(|_| rquickjs::Error::Unknown)?;
                context.spawn(run(context.clone(), receiver, stopped_sender));
                Ok(())
            }
        })
        .await?;

    Ok((queue, stopped_receiver))
}

async fn run<'js>(
    context: Ctx<'js>,
    mut tasks: UnboundedReceiver<MacrotaskMessage>,
    stopped: oneshot::Sender<()>,
) {
    while let Some(message) = tasks.recv().await {
        let task = match message {
            MacrotaskMessage::Run(task) => task,
            MacrotaskMessage::Shutdown(acknowledge) => {
                if let Some(acknowledge) = acknowledge {
                    let _ = acknowledge.send(());
                }
                break;
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
