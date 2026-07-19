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

#[tokio::test(flavor = "current_thread")]
async fn set_timeout_is_a_macrotask_and_promises_are_microtasks() {
    let runtime = Runtime::new().await.unwrap();
    let value: String = runtime
        .eval_promise(
            r#"
            (async () => {
                const order = [];
                setTimeout(() => {
                    order.push("macrotask-1");
                    Promise.resolve().then(() => order.push("microtask"));
                }, 10);
                setTimeout(() => order.push("macrotask-2"), 10);
                await new Promise(resolve => setTimeout(resolve, 20));
                return order.join(",");
            })()
            "#,
        )
        .await
        .unwrap();

    assert_eq!(value, "macrotask-1,microtask,macrotask-2");
}
