pub mod bridge;
mod document;
mod layout;
mod paint;

pub use document::UiDocument;
pub use layout::{LayoutError, UiLayout};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_react_snapshot_and_paints_elements() {
        let document = UiDocument::from_json(
            r##"{
                "root": {
                    "id": 1,
                    "type": "div",
                    "style": {
                        "display": "flex",
                        "flexDirection": "column",
                        "width": 300,
                        "padding": 16,
                        "gap": 12,
                        "backgroundColor": [245, 247, 250, 255]
                    },
                    "children": [
                        { "id": 2, "type": "text", "text": "Hello from React", "style": { "fontSize": 24, "lineHeight": 30 } },
                        { "id": 3, "type": "button", "style": { "padding": 8, "backgroundColor": [40, 80, 220, 255] }, "children": [
                            { "id": 4, "type": "text", "text": "Continue", "style": { "color": [255, 255, 255, 255] } }
                        ] }
                    ]
                }
            }"##,
        )
        .expect("valid UI snapshot");
        let mut text_system = render::TextSystem::new();
        let layout =
            UiLayout::compute(&document, 800.0, 600.0, &mut text_system).expect("Taffy layout");
        let size = layout.root_size().expect("root layout");
        assert_eq!(size.width, 300.0);
        assert!(size.height > 60.0);
        let canvas = layout.paint(render::Color::WHITE).expect("paint canvas");
        assert_eq!(canvas.commands().len(), 4);
    }
}
