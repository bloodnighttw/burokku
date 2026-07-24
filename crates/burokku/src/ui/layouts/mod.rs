//! Computed UI nodes ready for rendering and hit testing.
//! This should be used inside main threads.

mod compute;
mod iter;
mod stacking;
#[cfg(test)]
mod tests;

use render::{BoxStyle, TextStyle, TextSystem};

use crate::ui::elements::Document;

pub use iter::{LayoutIter, ReverseLayoutIter};
pub use stacking::StackingLayer;

/// Computes a renderable layout tree from an element document.
pub fn compute_layout(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
) -> Layout {
    compute::compute_layout(document, viewport_width, viewport_height, text_system)
}

/// The computed geometry and contents of an element.
///
/// Coordinates and dimensions are expressed in logical CSS pixels. `x` and
/// `y` are absolute viewport coordinates so consumers do not need to
/// accumulate parent offsets while rendering or hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// ID of the source node in [`crate::ui::elements::Document`].
    pub element_id: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: LayoutKind,
}

/// The renderable contents represented by a [`Layout`].
///
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutKind {
    Box {
        style: BoxStyle,
        stacking_layer: StackingLayer,
        children: Vec<Layout>,
    },
    Text {
        text: String,
        style: TextStyle,
    },
}

impl Layout {
    pub fn element_id(&self) -> u64 {
        self.element_id
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.width > 0.0
            && self.height > 0.0
            && x >= self.x
            && x < self.x + self.width
            && y >= self.y
            && y < self.y + self.height
    }

    /// Iterates over this layout and its descendants in render order.
    pub fn iter(&self) -> LayoutIter<'_> {
        LayoutIter::new(self)
    }

    /// Iterates over this layout and its descendants in reverse render order.
    ///
    /// This order is suitable for hit testing or event targeting because the
    /// visually topmost layout is visited first.
    pub fn iter_rev(&self) -> ReverseLayoutIter<'_> {
        ReverseLayoutIter::new(self)
    }

    pub fn stacking_layer(&self) -> StackingLayer {
        match &self.kind {
            LayoutKind::Box { stacking_layer, .. } => *stacking_layer,
            LayoutKind::Text { .. } => StackingLayer::default(),
        }
    }

    pub fn children(&self) -> &[Layout] {
        self.kind.children()
    }

    /// Returns the topmost layout containing the point.
    ///
    /// Hit testing follows reverse render order, including z-index and
    /// stacking-context boundaries.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&Layout> {
        self.iter_rev().find(|layout| layout.contains(x, y))
    }
}

impl<'a> IntoIterator for &'a Layout {
    type Item = &'a Layout;
    type IntoIter = LayoutIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl LayoutKind {
    pub fn children(&self) -> &[Layout] {
        match self {
            Self::Box { children, .. } => children,
            Self::Text { .. } => &[],
        }
    }
}
