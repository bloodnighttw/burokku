use super::{Layout, LayoutKind};

pub(super) trait Stacking {
    fn z_index(&self) -> Option<i32>;

    fn is_isolated(&self) -> bool;

    fn is_positioned(&self) -> bool;

    fn is_flex_or_grid_item(&self) -> bool;

    // determine that this stacking need to create new stacking context or not.
    fn establishes_stacking_context(&self) -> bool;

    fn stacking_index(&self) -> i32 {
        self.z_index().unwrap_or(0)
    }
}

impl Stacking for Layout {
    fn z_index(&self) -> Option<i32> {
        match &self.kind {
            LayoutKind::Box { z_index, .. } => *z_index,
            LayoutKind::Text { .. } => None,
        }
    }

    fn is_isolated(&self) -> bool {
        match &self.kind {
            LayoutKind::Box { isolated, .. } => *isolated,
            LayoutKind::Text { .. } => false,
        }
    }

    fn is_positioned(&self) -> bool {
        match &self.kind {
            LayoutKind::Box { positioned, .. } => *positioned,
            LayoutKind::Text { .. } => false,
        }
    }

    fn is_flex_or_grid_item(&self) -> bool {
        match &self.kind {
            LayoutKind::Box {
                flex_or_grid_item, ..
            } => *flex_or_grid_item,
            LayoutKind::Text { .. } => false,
        }
    }

    fn establishes_stacking_context(&self) -> bool {
        let creates_effect_context = match &self.kind {
            LayoutKind::Box { style, .. } => {
                style.opacity < 1.0 || style.transform != Default::default()
            }
            LayoutKind::Text { .. } => false,
        };

        let creates_indexed_context =
            self.z_index().is_some() && (self.is_positioned() || self.is_flex_or_grid_item());

        creates_indexed_context || self.is_isolated() || creates_effect_context
    }
}

/// Finds the stacking contexts that participate directly in `root`.
///
/// Traversal stops at each context boundary because its descendants belong to
/// that context, not the surrounding one. Stable sorting preserves document
/// order when two contexts use the same z-index.
pub(super) fn descendant_contexts(root: &Layout) -> Vec<&Layout> {
    let mut contexts = Vec::new();
    let mut pending = vec![root.children().iter()];

    while let Some(mut children) = pending.pop() {
        if let Some(layout) = children.next() {
            pending.push(children);
            if layout.establishes_stacking_context() {
                contexts.push(layout);
            } else {
                pending.push(layout.children().iter());
            }
        }
    }

    contexts.sort_by_key(|layout| layout.stacking_index());
    contexts
}

#[cfg(test)]
mod tests {
    use render::TextSystem;

    use crate::ui::elements::{Document, ElementKind, BODY_ID};

    use super::*;
    use crate::ui::layouts::compute_layout;

    fn styled_child(parent_display: Option<&str>, styles: &[(&str, &str)]) -> Layout {
        let mut document = Document::new();
        let parent = parent_display.map(|display| {
            let parent = document.create_node(ElementKind::Div);
            document
                .set_style(parent, "display", Some(display))
                .unwrap();
            document.insert(BODY_ID, parent, None).unwrap();
            parent
        });
        let child = document.create_node(ElementKind::Div);
        for (property, value) in styles {
            document.set_style(child, property, Some(value)).unwrap();
        }
        document
            .insert(parent.unwrap_or(BODY_ID), child, None)
            .unwrap();

        let layout = compute_layout(&document, 100.0, 100.0, &mut TextSystem::new());
        let parent = layout
            .children()
            .first()
            .expect("document should contain a child");
        parent_display
            .map_or(parent, |_: &str| {
                parent
                    .children()
                    .first()
                    .expect("parent should contain the styled child")
            })
            .clone()
    }

    #[test]
    fn default_box_does_not_establish_a_context() {
        let child = styled_child(None, &[]);

        assert!(!child.establishes_stacking_context());
    }

    #[test]
    fn z_index_requires_positioning_or_a_flex_or_grid_item() {
        let static_box = styled_child(None, &[("z-index", "1")]);
        let positioned_box = styled_child(None, &[("position", "relative"), ("z-index", "1")]);
        let flex_item = styled_child(Some("flex"), &[("z-index", "1")]);
        let grid_item = styled_child(Some("grid"), &[("z-index", "1")]);

        assert!(!static_box.establishes_stacking_context());
        assert!(positioned_box.establishes_stacking_context());
        assert!(flex_item.establishes_stacking_context());
        assert!(grid_item.establishes_stacking_context());
    }

    #[test]
    fn opacity_and_transforms_establish_contexts() {
        let opacity = styled_child(None, &[("opacity", "0.5")]);
        let transformed = styled_child(None, &[("transform", "translateX(4px)")]);

        assert!(opacity.establishes_stacking_context());
        assert!(transformed.establishes_stacking_context());
    }
}
