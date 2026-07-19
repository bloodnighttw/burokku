use runtime::Runtime;

#[tokio::test(flavor = "current_thread")]
async fn runs_javascript_event_loop_script() {
    let runtime = Runtime::new().await.unwrap();
    let source = include_str!("scripts/event_loop.js");
    let value: String = runtime.eval_promise(source).await.unwrap();

    assert_eq!(value, "macrotask-1,microtask,macrotask-2");
}
