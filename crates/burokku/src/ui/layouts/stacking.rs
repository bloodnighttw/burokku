use crate::ui::elements::styles::{Isolation, Style as ElementStyle, ZIndex};

use super::Layout;

/// The stacking properties attached to a rendered box.
///
/// A numeric z-index or isolation creates a stacking context. An isolated box
/// with an automatic z-index participates at layer zero in its parent context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StackingLayer {
    z_index: Option<i32>,
    isolated: bool,
}

impl StackingLayer {
    pub const fn new(z_index: Option<i32>, isolated: bool) -> Self {
        Self { z_index, isolated }
    }

    pub const fn z_index(self) -> Option<i32> {
        self.z_index
    }

    pub const fn is_isolated(self) -> bool {
        self.isolated
    }

    /// Whether this layer establishes a new stacking context.
    ///
    /// Burokku currently supports the two context-creating style conditions:
    /// a numeric `z-index` and `isolation: isolate`. Add future conditions
    /// such as opacity or transforms here when their styles are implemented.
    pub const fn establishes_context(self) -> bool {
        self.z_index.is_some() || self.isolated
    }

    /// Compatibility alias for [`Self::establishes_context`].
    pub const fn creates_context(self) -> bool {
        self.establishes_context()
    }

    pub const fn index(self) -> i32 {
        match self.z_index {
            Some(index) => index,
            None => 0,
        }
    }

    pub(crate) fn from_style(style: &ElementStyle) -> Self {
        let z_index = match style.z_index {
            ZIndex::Auto => None,
            ZIndex::Value(index) => Some(index),
        };
        let isolated = style.isolation == Isolation::Isolate;
        Self::new(z_index, isolated)
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
            if layout.stacking_layer().establishes_context() {
                contexts.push(layout);
            } else {
                pending.push(layout.children().iter());
            }
        }
    }

    contexts.sort_by_key(|layout| layout.stacking_layer().index());
    contexts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_z_index_establishes_a_context() {
        let style = ElementStyle {
            z_index: ZIndex::Value(3),
            ..ElementStyle::default()
        };

        assert!(StackingLayer::from_style(&style).establishes_context());
    }

    #[test]
    fn isolation_establishes_a_context_at_layer_zero() {
        let style = ElementStyle {
            isolation: Isolation::Isolate,
            ..ElementStyle::default()
        };
        let layer = StackingLayer::from_style(&style);

        assert!(layer.establishes_context());
        assert_eq!(layer.index(), 0);
    }

    #[test]
    fn automatic_styles_do_not_establish_a_context() {
        let layer = StackingLayer::from_style(&ElementStyle::default());

        assert!(!layer.establishes_context());
    }
}
