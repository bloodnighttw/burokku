use crate::ui::elements::styles::Position;
use crate::ui::elements::BODY_ID;

use super::{Layout, LayoutKind};

pub(crate) trait Stacking {
    fn is_root(&self) -> bool;

    fn z_index(&self) -> Option<i32>;

    fn is_isolated(&self) -> bool;

    fn position(&self) -> Option<Position>;

    fn is_positioned(&self) -> bool {
        self.position().is_some_and(Position::is_positioned)
    }

    fn is_flex_or_grid_item(&self) -> bool;

    // determine that this stacking need to create new stacking context or not.
    fn establishes_stacking_context(&self) -> bool;

    fn stacking_index(&self) -> i32 {
        if self.is_positioned() || self.is_flex_or_grid_item() {
            self.z_index().unwrap_or(0)
        } else {
            0
        }
    }

    fn is_positioned_auto(&self) -> bool {
        self.position()
            .is_some_and(|position| matches!(position, Position::Relative | Position::Absolute))
            && self.z_index().is_none()
    }
}

impl Stacking for Layout {
    fn is_root(&self) -> bool {
        self.element_id == BODY_ID
    }

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

    fn position(&self) -> Option<Position> {
        match &self.kind {
            LayoutKind::Box { position, .. } => Some(*position),
            LayoutKind::Text { .. } => None,
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
            LayoutKind::Box {
                style,
                has_transform,
                ..
            } => style.opacity < 1.0 || *has_transform,
            LayoutKind::Text {
                style,
                has_transform,
                ..
            } => style.opacity < 1.0 || *has_transform,
        };

        let creates_indexed_context =
            self.z_index().is_some() && (self.is_positioned() || self.is_flex_or_grid_item());
        let creates_fixed_context = self.position() == Some(Position::Fixed);

        self.is_root()
            || creates_indexed_context
            || creates_fixed_context
            || self.is_isolated()
            || creates_effect_context
    }
}

/// Finds the stacking contexts that participate directly in `root`.
///
/// Traversal stops at each context boundary because its descendants belong to
/// that context, not the surrounding one. Stable sorting preserves document
/// order when two contexts use the same z-index.
pub(crate) fn descendant_contexts(root: &Layout) -> Vec<&Layout> {
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

/// An entry in the zero stack level of `root`.
///
/// A real stacking context at stack level zero paints atomically. A
/// positioned box with `z-index: auto` paints in the same phase, but does not
/// establish a context: stacking-context descendants still participate in
/// `root`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ZeroLevelEntry<'a> {
    Context(&'a Layout),
    PositionedAuto(&'a Layout),
}

/// Finds zero-level contexts and positioned `z-index: auto` boxes in tree
/// order.
///
/// Traversal stops at real context boundaries. It continues through
/// positioned-auto boxes because their descendant contexts belong to `root`.
pub(crate) fn zero_level_entries(root: &Layout) -> Vec<ZeroLevelEntry<'_>> {
    let mut entries = Vec::new();
    let mut pending = vec![root.children().iter()];

    while let Some(mut children) = pending.pop() {
        if let Some(layout) = children.next() {
            pending.push(children);
            if layout.establishes_stacking_context() {
                if layout.stacking_index() == 0 {
                    entries.push(ZeroLevelEntry::Context(layout));
                }
                continue;
            }

            if layout.is_positioned_auto() {
                entries.push(ZeroLevelEntry::PositionedAuto(layout));
            }
            pending.push(layout.children().iter());
        }
    }

    entries
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

        assert!(!child.is_root());
        assert!(!child.establishes_stacking_context());
    }

    #[test]
    fn root_always_establishes_a_context() {
        let document = Document::new();
        let root = compute_layout(&document, 100.0, 100.0, &mut TextSystem::new());

        assert!(root.is_root());
        assert!(root.establishes_stacking_context());
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
    fn fixed_auto_establishes_a_context_but_absolute_auto_does_not() {
        let absolute = styled_child(None, &[("position", "absolute")]);
        let fixed = styled_child(None, &[("position", "fixed")]);

        assert!(!absolute.establishes_stacking_context());
        assert!(fixed.establishes_stacking_context());
    }

    #[test]
    fn opacity_and_transforms_establish_contexts() {
        let opacity = styled_child(None, &[("opacity", "0.5")]);
        let transformed = styled_child(None, &[("transform", "translateX(4px)")]);
        let identity_transform = styled_child(None, &[("transform", "translateX(0px)")]);

        assert!(opacity.establishes_stacking_context());
        assert!(transformed.establishes_stacking_context());
        assert!(identity_transform.establishes_stacking_context());
    }

    #[test]
    fn static_effect_context_ignores_its_z_index() {
        let opacity = styled_child(None, &[("opacity", "0.5"), ("z-index", "12")]);
        let transform = styled_child(None, &[("transform", "translateX(0px)"), ("z-index", "-4")]);
        let isolated = styled_child(None, &[("isolation", "isolate"), ("z-index", "8")]);

        for layout in [&opacity, &transform, &isolated] {
            assert!(layout.establishes_stacking_context());
            assert_eq!(layout.stacking_index(), 0);
        }
    }
}
