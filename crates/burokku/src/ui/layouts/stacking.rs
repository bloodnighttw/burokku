use super::{Layout, LayoutKind};

pub(super) trait Stacking {
    fn z_index(&self) -> Option<i32>;

    fn is_isolated(&self) -> bool;

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

    fn establishes_stacking_context(&self) -> bool {
        let creates_effect_context = match &self.kind {
            LayoutKind::Box { style, .. } => {
                style.opacity < 1.0 || style.transform != Default::default()
            }
            LayoutKind::Text { .. } => false,
        };

        self.z_index().is_some() || self.is_isolated() || creates_effect_context
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

    fn styled_child(property: &str, value: &str) -> Layout {
        let mut document = Document::new();
        let child = document.create_node(ElementKind::Div);
        document.set_style(child, property, Some(value)).unwrap();
        document.insert(BODY_ID, child, None).unwrap();

        compute_layout(&document, 100.0, 100.0, &mut TextSystem::new())
            .children()
            .first()
            .cloned()
            .expect("document should contain the styled child")
    }

    #[test]
    fn default_box_does_not_establish_a_context() {
        let child = styled_child("display", "block");

        assert!(!child.establishes_stacking_context());
    }

    #[test]
    fn opacity_and_transforms_establish_contexts() {
        let opacity = styled_child("opacity", "0.5");
        let transformed = styled_child("transform", "translateX(4px)");

        assert!(opacity.establishes_stacking_context());
        assert!(transformed.establishes_stacking_context());
    }
}
