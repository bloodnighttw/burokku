//! Computed UI nodes ready for rendering and hit testing.

use render::{BoxStyle, TextStyle};

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

    /// Returns the topmost layout containing the point.
    ///
    /// Children are stored in paint order, so hit testing visits them in
    /// reverse order.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&Layout> {
        if !self.contains(x, y) {
            return None;
        }

        if let LayoutKind::Box { children, .. } = &self.kind {
            for child in children.iter().rev() {
                if let Some(hit) = child.hit_test(x, y) {
                    return Some(hit);
                }
            }
        }

        Some(self)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text_layout(element_id: u64, x: f32, y: f32) -> Layout {
        Layout {
            element_id,
            x,
            y,
            width: 80.0,
            height: 20.0,
            kind: LayoutKind::Text {
                text: "Burokku".into(),
                style: TextStyle::default(),
            },
        }
    }

    #[test]
    fn layout_can_be_transferred_through_a_channel() {
        let layout = text_layout(7, 10.0, 20.0);
        let (sender, receiver) = std::sync::mpsc::channel();

        sender.send(layout).unwrap();

        assert_eq!(receiver.recv().unwrap().element_id(), 7);
    }

    #[test]
    fn hit_testing_returns_the_topmost_child() {
        let layout = Layout {
            element_id: 1,
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
            kind: LayoutKind::Box {
                style: BoxStyle::default(),
                children: vec![text_layout(2, 20.0, 20.0), text_layout(3, 20.0, 20.0)],
            },
        };

        assert_eq!(layout.hit_test(30.0, 30.0).unwrap().element_id(), 3);
        assert!(layout.hit_test(300.0, 300.0).is_none());
    }
}
