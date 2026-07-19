use super::Runtime;

#[tokio::test(flavor = "current_thread")]
async fn evaluates_javascript() {
    let runtime = Runtime::new().await.unwrap();
    let value: i32 = runtime.eval("20 + 22").await.unwrap();

    assert_eq!(value, 42);
}

#[tokio::test(flavor = "current_thread")]
async fn resolves_a_promise() {
    let runtime = Runtime::new().await.unwrap();
    let value: String = runtime
        .eval_promise("Promise.resolve('Burokku')")
        .await
        .unwrap();

    assert_eq!(value, "Burokku");
}
