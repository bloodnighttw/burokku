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
}
