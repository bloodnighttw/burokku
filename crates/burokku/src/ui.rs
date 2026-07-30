mod bridge;
mod render;
mod store;

pub mod elements;
pub mod layouts;

pub use bridge::install;
pub use elements::{Document, DocumentError, ElementKind};
pub use render::{build_canvas, UiFrame};
pub(crate) use render::{build_frame_with_scroll, repaint_frame};
pub use store::UiStore;

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::Runtime;

    #[tokio::test(flavor = "current_thread")]
    async fn javascript_dom_facade_drives_the_ui_document() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        runtime
            .eval::<()>(
                r##"
                const card = document.createElement("div");
                card.style.display = "flex";
                card.style.backgroundColor = "#102030";
                const label = document.createTextNode("before");
                card.appendChild(label);
                document.body.appendChild(card);
                label.data = "after";
                card.removeChild(label);
                card.appendChild(label);
                "##,
            )
            .await
            .unwrap();

        let snapshot = store.snapshot();
        let card_id = snapshot.body().children[0];
        let card = snapshot.node(card_id).unwrap();
        assert_eq!(card.kind, ElementKind::Div);
        assert_eq!(card.style.background_color, Some([16, 32, 48, 255]));
        assert_eq!(
            snapshot.node(card.children[0]).unwrap().kind,
            ElementKind::Text("after".into())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejected_dom_insert_preserves_the_javascript_and_native_trees() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        runtime
            .eval::<()>(
                r##"
                const parent = document.createElement("div");
                const child = document.createElement("div");
                document.body.appendChild(parent);
                parent.appendChild(child);

                let rejected = false;
                try {
                  child.appendChild(parent);
                } catch {
                  rejected = true;
                }

                if (!rejected) throw new Error("cyclic insertion should be rejected");
                if (parent.parentNode !== document.body) {
                  throw new Error("rejected insertion moved the parent");
                }
                if (child.parentNode !== parent) {
                  throw new Error("rejected insertion detached the child");
                }
                if (document.body.firstChild !== parent || parent.firstChild !== child) {
                  throw new Error("rejected insertion changed child order");
                }
                "##,
            )
            .await
            .unwrap();

        let snapshot = store.snapshot();
        let parent_id = snapshot.body().children[0];
        let parent = snapshot.node(parent_id).unwrap();
        let child_id = parent.children[0];

        assert_eq!(parent.parent, Some(elements::BODY_ID));
        assert_eq!(snapshot.node(child_id).unwrap().parent, Some(parent_id));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn moving_connected_node_into_fragment_detaches_the_native_node() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        runtime
            .eval::<()>(
                r##"
                globalThis.__testMovedNode = document.createElement("div");
                document.body.appendChild(globalThis.__testMovedNode);
                "##,
            )
            .await
            .unwrap();

        let child_id = store.snapshot().body().children[0];
        runtime
            .eval::<()>(
                r##"
                globalThis.__testFragment = document.createDocumentFragment();
                globalThis.__testFragment.appendChild(globalThis.__testMovedNode);

                if (document.body.firstChild !== null) {
                  throw new Error("fragment move left the child under the body");
                }
                if (globalThis.__testMovedNode.parentNode !== globalThis.__testFragment) {
                  throw new Error("fragment did not become the JavaScript parent");
                }
                if (globalThis.__testMovedNode.isConnected) {
                  throw new Error("node in a fragment should be disconnected");
                }
                "##,
            )
            .await
            .unwrap();

        let detached = store.snapshot();
        assert!(detached.body().children.is_empty());
        assert_eq!(detached.node(child_id).unwrap().parent, None);

        runtime
            .eval::<()>(
                r##"
                document.body.appendChild(globalThis.__testFragment);
                if (document.body.firstChild !== globalThis.__testMovedNode) {
                  throw new Error("fragment child was not reattached");
                }
                if (globalThis.__testFragment.firstChild !== null) {
                  throw new Error("inserted fragment should be empty");
                }
                "##,
            )
            .await
            .unwrap();

        let reattached = store.snapshot();
        assert_eq!(reattached.body().children, [child_id]);
        assert_eq!(
            reattached.node(child_id).unwrap().parent,
            Some(elements::BODY_ID)
        );
    }
}
