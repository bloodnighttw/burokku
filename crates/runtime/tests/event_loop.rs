use runtime::Runtime;

#[tokio::test(flavor = "current_thread")]
async fn runs_javascript_event_loop_script() {
    let runtime = Runtime::new().await.unwrap();
    let source = include_str!("scripts/event_loop.js");
    let value: String = runtime.eval_promise(source).await.unwrap();

    assert_eq!(value, "macrotask-1,microtask,macrotask-2");
}

#[tokio::test(flavor = "current_thread")]
async fn set_interval_repeats_as_macrotasks() {
    let runtime = Runtime::new().await.unwrap();
    let source = include_str!("scripts/set_interval.js");
    let value: String = runtime.eval_promise(source).await.unwrap();

    assert_eq!(value, "interval-1,microtask,interval-2,interval-3");
}

#[tokio::test(flavor = "current_thread")]
async fn clear_timer_functions_cancel_callbacks() {
    let runtime = Runtime::new().await.unwrap();
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
}
