mod bridge;
mod render;
mod store;

pub mod computed;
pub mod elements;

pub use bridge::install;
pub use elements::Elements;
pub use render::build_canvas;
pub use store::UiStore;

#[cfg(test)]
mod tests {
    use runtime::Runtime;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn javascript_render_publishes_a_complete_element_tree() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();

        runtime
            .eval::<()>(
                r##"
                __burokku_render(JSON.stringify({
                  type: "app",
                  children: [{
                    type: "window",
                    children: [{
                      type: "text",
                      style: { color: "#102030", fontSize: 20 },
                      children: [{ type: "string", value: "hello" }]
                    }]
                  }]
                }));
                "##,
            )
            .await
            .unwrap();

        let snapshot = Elements::from_json(&store.snapshot()).unwrap();
        let Elements::App { children } = &snapshot else {
            panic!("the host root must be an app");
        };
        assert!(matches!(children.as_slice(), [Elements::Window { .. }]));
        assert_eq!(store.version(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_render_does_not_replace_the_previous_tree() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();

        let result = runtime
            .eval::<()>(r#"__burokku_render('{\"type\":\"unknown\"}')"#)
            .await;

        assert!(result.is_err());
        assert_eq!(store.version(), 0);
    }
}
