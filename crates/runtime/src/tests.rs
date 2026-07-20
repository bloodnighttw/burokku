use super::{InputState, ModifiersState, Runtime, WindowEventMessage};

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
async fn dispatches_native_events_as_macrotasks() {
    let runtime = Runtime::new().await.unwrap();
    runtime
        .eval::<()>(
            r#"
            globalThis.__events = [];
            globalThis.__burokku_dispatch_event = event => __events.push(event.type);
            "#,
        )
        .await
        .unwrap();

    runtime
        .enqueue_window_events(&[
            WindowEventMessage::Resized {
                width: 800,
                height: 600,
            },
            WindowEventMessage::CloseRequested,
        ])
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let events: Vec<String> = runtime.eval("__events").await.unwrap();
    assert_eq!(events, ["resized", "close-requested"]);
}

#[tokio::test(flavor = "current_thread")]
async fn includes_native_input_event_data() {
    let runtime = Runtime::new().await.unwrap();
    runtime
        .eval::<()>(
            r#"
            globalThis.__events = [];
            globalThis.__burokku_dispatch_event = event => __events.push(event);
            "#,
        )
        .await
        .unwrap();

    runtime
        .enqueue_window_events(&[
            WindowEventMessage::CursorMoved { x: 12.5, y: 24.0 },
            WindowEventMessage::KeyboardInput {
                key_code: 0,
                text: Some("a".into()),
                state: InputState::Pressed,
                repeat: false,
                modifiers: ModifiersState {
                    shift: true,
                    ..ModifiersState::default()
                },
            },
        ])
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let json: String = runtime.eval("JSON.stringify(__events)").await.unwrap();
    assert_eq!(
        json,
        r#"[{"type":"cursor-moved","x":12.5,"y":24},{"type":"keyboard-input","keyCode":0,"text":"a","pressed":true,"repeat":false,"shiftKey":true,"ctrlKey":false,"altKey":false,"metaKey":false,"capsLock":false}]"#
    );
}
