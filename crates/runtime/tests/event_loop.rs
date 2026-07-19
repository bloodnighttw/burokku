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
