use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tokio::runtime::Builder;
use tokio::task::LocalSet;

#[test]
fn quickjs_driver_progresses_on_external_tokio_main_thread() {
    let (wake_tx, wake_rx) = mpsc::channel();
    let tokio = Builder::new_current_thread()
        .enable_all()
        .external_event_loop(Arc::new(move || {
            let _ = wake_tx.send(());
        }))
        .external_tick_budget(32)
        .build()
        .unwrap();
    let owner = std::thread::current().id();
    let mut local = LocalSet::new();
    let (done_tx, done_rx) = mpsc::channel();

    local.spawn_local(async move {
        assert_eq!(std::thread::current().id(), owner);
        let (javascript, driver) = runtime::Runtime::builder().build_driven().await.unwrap();
        let driver = tokio::task::spawn_local(driver.run());

        let value = javascript.eval::<i32>("6 * 7").await.unwrap();
        assert_eq!(std::thread::current().id(), owner);
        javascript.shutdown().await.unwrap();
        driver.await.unwrap();
        done_tx.send(value).unwrap();
    });

    let timeout = Instant::now() + Duration::from_secs(5);
    let value = loop {
        if let Ok(value) = done_rx.try_recv() {
            break value;
        }
        assert!(Instant::now() < timeout, "QuickJS driver stalled");
        let tick = tokio.tick_nonblocking_with_local_set(&mut local);
        if !tick.has_more_work {
            let wait = tick
                .next_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_millis(50))
                .min(Duration::from_millis(50));
            let _ = wake_rx.recv_timeout(wait);
        }
    };

    assert_eq!(value, 42);
}
