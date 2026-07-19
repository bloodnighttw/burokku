pub mod bridge;
mod document;
mod layout;
mod paint;

pub use document::{UiDocument, UiUpdate};
pub use layout::{LayoutError, UiLayout};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_incremental_react_document_and_paints_elements() {
        use document::{ElementKind, UiMutation, UiStyleValue};

        let mut document = UiDocument::new();
        for (id, kind) in [
            (1, ElementKind::Div),
            (2, ElementKind::Text),
            (3, ElementKind::Button),
            (4, ElementKind::Text),
        ] {
            document
                .apply_mutation(UiMutation::Create { id, kind })
                .unwrap();
        }
        for (name, value) in [
            ("width", UiStyleValue::Number(300.0)),
            ("padding", UiStyleValue::Number(16.0)),
            ("gap", UiStyleValue::Number(12.0)),
            ("backgroundColor", UiStyleValue::Color([245, 247, 250, 255])),
        ] {
            document
                .apply_mutation(UiMutation::SetStyle {
                    id: 1,
                    name: name.into(),
                    value,
                })
                .unwrap();
        }
        document
            .apply_mutation(UiMutation::SetStyle {
                id: 3,
                name: "backgroundColor".into(),
                value: UiStyleValue::Color([40, 80, 220, 255]),
            })
            .unwrap();
        for (id, text) in [(2, "Hello from React"), (4, "Continue")] {
            document
                .apply_mutation(UiMutation::SetText {
                    id,
                    text: text.into(),
                })
                .unwrap();
        }
        for (parent, child) in [(0, 1), (1, 2), (1, 3), (3, 4)] {
            document
                .apply_mutation(UiMutation::Insert {
                    parent,
                    child,
                    before: None,
                })
                .unwrap();
        }
        let mut text_system = render::TextSystem::new();
        let layout =
            UiLayout::compute(&document, 800.0, 600.0, &mut text_system).expect("Taffy layout");
        let size = layout.root_size().expect("root layout");
        assert_eq!(size.width, 800.0);
        assert_eq!(size.height, 600.0);
        let canvas = layout.paint(render::Color::WHITE).expect("paint canvas");
        assert_eq!(canvas.commands().len(), 4);

        document
            .apply_mutation(UiMutation::Remove {
                parent: 1,
                child: 3,
            })
            .unwrap();
        assert!(document.node(3).is_err());
        assert!(document.node(4).is_err());
    }
}
