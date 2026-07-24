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
    async fn javascript_select_properties_update_native_selection_state() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        let selected = runtime
            .eval::<String>(
                r##"
                const select = document.createElement("select");
                const first = document.createElement("option");
                first.value = "first";
                first.textContent = "First";
                const second = document.createElement("option");
                second.value = "second";
                second.textContent = "Second";
                select.append(first, second);
                document.body.append(select);
                select.value = "second";
                `${select.value}:${select.selectedIndex}`;
                "##,
            )
            .await
            .unwrap();

        assert_eq!(selected, "second:1");
        let snapshot = store.snapshot();
        let select = snapshot.node(snapshot.body().children[0]).unwrap();
        assert_eq!(select.kind, ElementKind::Select);
        assert_eq!(
            snapshot.node(select.children[0]).unwrap().kind,
            ElementKind::Option
        );
        assert!(snapshot
            .node(select.children[1])
            .unwrap()
            .attributes
            .contains_key("data-burokku-selected"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn single_select_state_does_not_fall_back_after_explicit_deselection() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        let state = runtime
            .eval::<String>(
                r##"
                const select = document.createElement("select");
                const first = document.createElement("option");
                first.value = "first";
                const second = document.createElement("option");
                second.value = "second";
                second.selected = true;
                select.append(first, second);
                document.body.append(select);
                const fallback = `${select.value}:${select.selectedIndex}`;
                first.selected = true;
                second.selected = true;
                const exclusive = `${first.selected}:${second.selected}`;
                select.value = "missing";
                const empty = `${select.value}:${select.selectedIndex}:${first.selected}:${second.selected}`;
                `${fallback}|${exclusive}|${empty}`;
                "##,
            )
            .await
            .unwrap();

        assert_eq!(state, "second:1|false:true|:-1:false:false");
        let snapshot = store.snapshot();
        let select = snapshot.node(snapshot.body().children[0]).unwrap();
        assert!(select
            .attributes
            .contains_key("data-burokku-selection-explicit"));
        assert!(select.children.iter().all(|option| !snapshot
            .node(*option)
            .unwrap()
            .attributes
            .contains_key("data-burokku-selected")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multiple_select_preserves_independent_option_selection() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        let state = runtime
            .eval::<String>(
                r##"
                const select = document.createElement("select");
                select.multiple = true;
                const disabled = document.createElement("option");
                disabled.value = "disabled";
                disabled.disabled = true;
                const first = document.createElement("option");
                first.value = "first";
                const second = document.createElement("option");
                second.value = "second";
                select.append(disabled, first, second);
                first.selected = true;
                second.selected = true;
                `${disabled.selected}:${first.selected}:${second.selected}:${select.value}:${select.selectedIndex}`;
                "##,
            )
            .await
            .unwrap();

        assert_eq!(state, "false:true:true:first:1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn focus_state_is_reflected_and_disabled_controls_reject_focus() {
        let store = UiStore::new();
        let host_store = store.clone();
        let runtime = Runtime::new_with_host(move |context| install(context, host_store))
            .await
            .unwrap();
        let state = runtime
            .eval::<String>(
                r##"
                const enabled = document.createElement("button");
                const disabled = document.createElement("button");
                disabled.disabled = true;
                document.body.append(enabled, disabled);
                enabled.focus();
                const focused = enabled.hasAttribute("data-burokku-focused");
                disabled.focus();
                `${focused}:${document.activeElement === enabled}:${disabled.hasAttribute("data-burokku-focused")}`;
                "##,
            )
            .await
            .unwrap();

        assert_eq!(state, "true:true:false");
    }
}
