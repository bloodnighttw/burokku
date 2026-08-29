use runtime::{plugins::TimersPlugin, Runtime};
use tokio::task::LocalSet;

#[tokio::test(flavor = "current_thread")]
async fn runs_javascript_event_loop_script() {
    LocalSet::new()
        .run_until(async {
            let (runtime, driver) = Runtime::builder()
                .plugin(TimersPlugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let source = include_str!("scripts/event_loop.js");
            let value: String = runtime.eval_promise(source).await.unwrap();
            assert_eq!(value, "macrotask-1,microtask,macrotask-2");
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn set_interval_repeats_as_macrotasks() {
    LocalSet::new()
        .run_until(async {
            let (runtime, driver) = Runtime::builder()
                .plugin(TimersPlugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let source = include_str!("scripts/set_interval.js");
            let value: String = runtime.eval_promise(source).await.unwrap();
            assert_eq!(value, "interval-1,microtask,interval-2,interval-3");
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn clear_timer_functions_cancel_callbacks() {
    LocalSet::new()
        .run_until(async {
            let (runtime, driver) = Runtime::builder()
                .plugin(TimersPlugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let value: i32 = runtime
                .eval_promise(
                    r#"new Promise(resolve => {
                        let calls = 0;
                        const interval = setInterval(() => calls += 1, 0);
                        const timeout = setTimeout(() => calls += 100, 0);
                        clearInterval(interval);
                        clearTimeout(timeout);
                        setTimeout(() => resolve(calls), 5);
                    })"#,
                )
                .await
                .unwrap();
            assert_eq!(value, 0);
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}
