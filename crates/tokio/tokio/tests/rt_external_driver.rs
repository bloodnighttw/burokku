#![cfg(all(feature = "rt", feature = "net", feature = "time"))]

use std::cell::Cell;
use std::io::Write;
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::runtime::{Builder, Runtime, TickResult};
use tokio::task::LocalSet;

fn drive_until(
    runtime: &Runtime,
    wake_rx: &mpsc::Receiver<()>,
    timeout: Duration,
    mut done: impl FnMut() -> bool,
) {
    let stop = Instant::now() + timeout;
    while !done() {
        assert!(
            Instant::now() < stop,
            "external runtime did not make progress"
        );
        let TickResult {
            has_more_work,
            next_deadline,
            ..
        } = runtime.tick_nonblocking();
        if has_more_work {
            continue;
        }

        let timer_wait = next_deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(50));
        let wait = timer_wait.min(Duration::from_millis(50));
        let _ = wake_rx.recv_timeout(wait);
    }
}

#[cfg(tokio_unstable)]
#[test]
fn unhandled_panic_restores_core_before_tick_panics() {
    use tokio::runtime::UnhandledPanic;

    let runtime = Builder::new_current_thread()
        .external_event_loop(Arc::new(|| {}))
        .unhandled_panic(UnhandledPanic::ShutdownRuntime)
        .build()
        .unwrap();
    runtime.spawn(async { panic!("boom") });

    let panic = std::panic::catch_unwind(|| runtime.tick_nonblocking());
    assert!(panic.is_err());
    // In particular, this must not panic with "We never placed the Core back".
    drop(runtime);
}

#[test]
fn bounded_tick_reports_remaining_work() {
    let wake = Arc::new(|| {});
    let runtime = Builder::new_current_thread()
        .external_event_loop(wake)
        .external_tick_budget(2)
        .build()
        .unwrap();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for _ in 0..5 {
        let count = count.clone();
        runtime.spawn(async move {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    }

    let first = runtime.tick_nonblocking();
    assert_eq!(first.tasks_polled, 2);
    assert!(first.has_more_work);
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 2);

    while runtime.tick_nonblocking().has_more_work {}
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 5);
}

#[test]
fn timer_deadline_is_reported_and_expires() {
    let (wake_tx, wake_rx) = mpsc::channel();
    let runtime = Builder::new_current_thread()
        .enable_time()
        .external_event_loop(Arc::new(move || {
            let _ = wake_tx.send(());
        }))
        .build()
        .unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        done_tx.send(()).unwrap();
    });

    let first = runtime.tick_nonblocking();
    let deadline = first.next_deadline.expect("timer wheel deadline");
    let remaining = deadline.saturating_duration_since(Instant::now());
    assert!(
        remaining <= Duration::from_millis(120),
        "deadline too late: {remaining:?}"
    );
    assert!(
        remaining >= Duration::from_millis(20),
        "deadline too early: {remaining:?}"
    );
    drive_until(&runtime, &wake_rx, Duration::from_secs(1), || {
        done_rx.try_recv().is_ok()
    });
}

#[test]
fn spawn_blocking_completion_wakes_external_loop() {
    let (wake_tx, wake_rx) = mpsc::channel();
    let runtime = Builder::new_current_thread()
        .external_event_loop(Arc::new(move || {
            let _ = wake_tx.send(());
        }))
        .build()
        .unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    runtime.spawn(async move {
        let value = tokio::task::spawn_blocking(move || {
            release_rx.recv().unwrap();
            42
        })
        .await
        .unwrap();
        done_tx.send(value).unwrap();
    });

    runtime.tick_nonblocking();
    while wake_rx.try_recv().is_ok() {}
    release_tx.send(()).unwrap();
    wake_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("blocking-pool completion did not signal ExternalWake");
    while done_rx.try_recv().is_err() {
        runtime.tick_nonblocking();
    }
}

#[test]
fn timer_and_network_progress_without_polling_mio_on_main() {
    let (wake_tx, wake_rx) = mpsc::channel();
    let wake = Arc::new(move || {
        let _ = wake_tx.send(());
    });
    let runtime = Builder::new_current_thread()
        .enable_all()
        .external_event_loop(wake)
        .external_tick_budget(8)
        .build()
        .unwrap();

    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            // Some hermetic test runners prohibit opening even loopback sockets.
            return;
        }
        Err(error) => panic!("failed to bind loopback test listener: {error}"),
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"ready").unwrap();
    });

    let main_thread = thread::current().id();
    let (done_tx, done_rx) = mpsc::channel();
    runtime.spawn(async move {
        assert_eq!(thread::current().id(), main_thread);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(thread::current().id(), main_thread);

        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let mut bytes = [0; 5];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"ready");
        assert_eq!(thread::current().id(), main_thread);
        done_tx.send(()).unwrap();
    });

    let first = runtime.tick_nonblocking();
    assert!(first.next_deadline.is_some());
    drive_until(&runtime, &wake_rx, Duration::from_secs(2), || {
        done_rx.try_recv().is_ok()
    });
    server.join().unwrap();
}

#[cfg(unix)]
#[test]
fn reactor_thread_publishes_unix_socket_readiness() {
    use std::io::Write as _;
    use std::os::unix::net::UnixStream as StdUnixStream;

    let (wake_tx, wake_rx) = mpsc::channel();
    let runtime = Builder::new_current_thread()
        .enable_io()
        .external_event_loop(Arc::new(move || {
            let _ = wake_tx.send(thread::current().id());
        }))
        .build()
        .unwrap();

    let (reader, mut writer) = StdUnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    let mut reader = {
        let _enter = runtime.enter();
        tokio::net::UnixStream::from_std(reader).unwrap()
    };
    let owner = thread::current().id();
    let (done_tx, done_rx) = mpsc::channel();
    runtime.spawn(async move {
        let mut byte = [0; 1];
        reader.read_exact(&mut byte).await.unwrap();
        assert_eq!(byte, [7]);
        assert_eq!(thread::current().id(), owner);
        done_tx.send(()).unwrap();
    });

    runtime.tick_nonblocking();
    while wake_rx.recv_timeout(Duration::from_millis(20)).is_ok() {}
    let writer = thread::spawn(move || writer.write_all(&[7]).unwrap());

    let stop = Instant::now() + Duration::from_secs(2);
    loop {
        let callback_thread = wake_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("Mio readiness did not signal ExternalWake");
        assert_ne!(callback_thread, owner, "readiness wake ran on main thread");
        runtime.tick_nonblocking();
        if done_rx.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < stop, "Unix readiness task stalled");
    }
    writer.join().unwrap();
}

#[cfg(unix)]
#[test]
fn shutdown_joins_reactor_with_outstanding_registration() {
    use std::os::unix::net::UnixStream as StdUnixStream;

    let runtime = Builder::new_current_thread()
        .enable_io()
        .external_event_loop(Arc::new(|| {}))
        .build()
        .unwrap();
    let (reader, _writer) = StdUnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    let reader = {
        let _enter = runtime.enter();
        tokio::net::UnixStream::from_std(reader).unwrap()
    };

    let started = Instant::now();
    drop(runtime);
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(reader);
}

#[test]
fn local_set_futures_stay_on_driving_thread() {
    let runtime = Builder::new_current_thread()
        .external_event_loop(Arc::new(|| {}))
        .build()
        .unwrap();
    let mut local = LocalSet::new();
    let value = Rc::new(Cell::new(0));
    let task_value = value.clone();
    let owner = thread::current().id();

    local.spawn_local(async move {
        assert_eq!(thread::current().id(), owner);
        task_value.set(42);
    });

    let result = runtime.tick_nonblocking_with_local_set(&mut local);
    assert_eq!(value.get(), 42);
    assert!(!result.has_more_work);
}
