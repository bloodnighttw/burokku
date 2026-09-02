use std::{
    cell::Cell,
    future::{pending, Future},
    rc::Rc,
    sync::{mpsc, Arc},
    task::{Context, Wake, Waker},
    time::{Duration, Instant},
};

use tokio::{runtime::Builder, task::LocalSet};

struct ChannelWaker(mpsc::Sender<()>);

impl Wake for ChannelWaker {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send(());
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.send(());
    }
}

fn poll_local(runtime: &tokio::runtime::Runtime, local: &LocalSet, waker: &Waker) {
    let _runtime = runtime.enter();
    let mut future = std::pin::pin!(local.run_until(pending::<()>()));
    let mut context = Context::from_waker(waker);
    assert!(future.as_mut().poll(&mut context).is_pending());
}

#[test]
fn quickjs_driver_progresses_on_upstream_tokio() {
    const BACKLOG: usize = 256;

    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let local = LocalSet::new();
    let owner = std::thread::current().id();
    let completed = Rc::new(Cell::new(0));

    for _ in 0..BACKLOG {
        let completed = Rc::clone(&completed);
        local.spawn_local(async move { completed.set(completed.get() + 1) });
    }

    let (done_tx, done_rx) = mpsc::channel();
    local.spawn_local(async move {
        assert_eq!(std::thread::current().id(), owner);
        let worker = tokio::spawn(async { std::thread::current().id() })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(std::thread::current().id(), owner);

        let (javascript, driver) = runtime::Runtime::builder().build_driven().await.unwrap();
        let driver = tokio::task::spawn_local(driver.run());
        let value = javascript.eval::<i32>("6 * 7").await.unwrap();
        javascript.shutdown().await.unwrap();
        driver.await.unwrap();
        done_tx.send((value, worker)).unwrap();
    });

    let (wake_tx, wake_rx) = mpsc::channel();
    let waker = Waker::from(Arc::new(ChannelWaker(wake_tx)));
    poll_local(&runtime, &local, &waker);
    assert!(completed.get() < BACKLOG, "backlog drained in one poll");
    wake_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("LocalSet backlog did not request another poll");

    let timeout = Instant::now() + Duration::from_secs(5);
    let (value, worker) = loop {
        if let Ok(result) = done_rx.try_recv() {
            break result;
        }
        assert!(Instant::now() < timeout, "QuickJS driver stalled");
        poll_local(&runtime, &local, &waker);
        let _ = wake_rx.recv_timeout(Duration::from_millis(50));
    };

    assert_eq!(completed.get(), BACKLOG);
    assert_eq!(value, 42);
    assert_ne!(worker, owner);
}
