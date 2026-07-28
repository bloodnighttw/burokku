//! Computed UI nodes ready for rendering and hit testing.
//! This should be used inside main threads.

mod compute;
mod iter;
mod stacking;

use render::{BoxStyle, Clip, Rect, TextRunMetrics, TextSpan, TextStyle, TextSystem};
use std::collections::HashMap;

use crate::ui::elements::Document;

pub use crate::ui::elements::styles::Position;

pub use iter::{LayoutIter, ReverseLayoutIter};
pub(crate) use stacking::{descendant_contexts, zero_level_entries, Stacking, ZeroLevelEntry};

/// Computes a renderable layout tree from an element document.
pub fn compute_layout(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
) -> Layout {
    compute::compute_layout(document, viewport_width, viewport_height, text_system)
}

pub(crate) fn compute_layout_with_scroll(
    document: &Document,
    viewport_width: f32,
    viewport_height: f32,
    text_system: &mut TextSystem,
    scroll_offsets: &HashMap<u64, ScrollOffset>,
) -> Layout {
    compute::compute_layout_with_scroll(
        document,
        viewport_width,
        viewport_height,
        text_system,
        scroll_offsets,
    )
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
    /// Cumulative affine transform around this layout's center.
    pub transform: render::Transform,
    /// Ancestor overflow clips in viewport coordinates, outermost first.
    pub clips: Vec<Clip>,
    /// Scroll geometry when this box establishes a scroll container.
    pub scroll: Option<ScrollContainer>,
    pub kind: LayoutKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollOffset {
    pub x: f32,
    pub y: f32,
}

impl ScrollOffset {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scrollbar {
    pub axis: ScrollbarAxis,
    pub track: Rect,
    pub thumb: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollContainer {
    pub viewport: Rect,
    pub clip: Clip,
    pub content_width: f32,
    pub content_height: f32,
    pub offset: ScrollOffset,
    pub max_offset: ScrollOffset,
    pub horizontal: Option<Scrollbar>,
    pub vertical: Option<Scrollbar>,
}

impl ScrollContainer {
    pub fn scrollbar_at(self, x: f32, y: f32) -> Option<Scrollbar> {
        [self.vertical, self.horizontal]
            .into_iter()
            .flatten()
            .find(|scrollbar| scrollbar.track.contains(x, y))
    }
}

/// The renderable contents represented by a [`Layout`].
///
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutKind {
    Box {
        style: BoxStyle,
        has_transform: bool,
        z_index: Option<i32>,
        isolated: bool,
        position: Position,
        fixed_containing_block: Option<u64>,
        fixed_to_viewport: bool,
        flex_or_grid_item: bool,
        children: Vec<Layout>,
    },
    Text {
        text: String,
        spans: Vec<TextSpan>,
        style: TextStyle,
        has_transform: bool,
        line_count: usize,
        runs: Vec<TextRunMetrics>,
    },
}

impl Layout {
    pub fn element_id(&self) -> u64 {
        self.element_id
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        let center = [self.x + self.width * 0.5, self.y + self.height * 0.5];
        let [a, b, c, d, tx, ty] = self.transform.matrix;
        let determinant = a * d - b * c;
        if determinant.abs() <= f32::EPSILON {
            return false;
        }
        let relative = [x - center[0] - tx, y - center[1] - ty];
        let local = [
            (d * relative[0] - c * relative[1]) / determinant + center[0],
            (-b * relative[0] + a * relative[1]) / determinant + center[1],
        ];
        self.clips.iter().all(|clip| clip.contains(x, y))
            && self.width > 0.0
            && self.height > 0.0
            && local[0] >= self.x
            && local[0] < self.x + self.width
            && local[1] >= self.y
            && local[1] < self.y + self.height
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

    pub fn children(&self) -> &[Layout] {
        self.kind.children()
    }

    pub fn scroll_container_at(&self, x: f32, y: f32) -> Option<&Layout> {
        self.iter_rev().find(|layout| {
            layout
                .scroll
                .is_some_and(|scroll| scroll.viewport.contains(x, y))
                && layout.clips.iter().all(|clip| clip.contains(x, y))
        })
    }

    /// Returns the topmost layout containing the point.
    ///
    /// Hit testing follows reverse render order, including z-index and
    /// stacking-context boundaries.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&Layout> {
        self.iter_rev().find(|layout| layout.contains(x, y))
    }

    /// Updates a retained scroll container without recomputing document layout.
    ///
    /// Scrolling changes descendant placement and scrollbar thumb geometry,
    /// but it does not affect the sizes produced by the layout engine.
    pub(crate) fn apply_scroll_offset(&mut self, element_id: u64, requested: ScrollOffset) -> bool {
        if self.element_id == element_id {
            return self.apply_own_scroll_offset(requested);
        }

        match &mut self.kind {
            LayoutKind::Box { children, .. } => children
                .iter_mut()
                .any(|child| child.apply_scroll_offset(element_id, requested)),
            LayoutKind::Text { .. } => false,
        }
    }

    fn apply_own_scroll_offset(&mut self, requested: ScrollOffset) -> bool {
        let Some(scroll) = &mut self.scroll else {
            return false;
        };
        let offset = ScrollOffset::new(
            requested.x.clamp(0.0, scroll.max_offset.x),
            requested.y.clamp(0.0, scroll.max_offset.y),
        );
        if offset == scroll.offset {
            return false;
        }

        let translation = ScrollOffset::new(scroll.offset.x - offset.x, scroll.offset.y - offset.y);
        // Descendant clip lists begin with this box's ancestor clips followed
        // by this scroll container's stationary viewport clip. Clips created
        // farther down the moving subtree must move with their owning boxes.
        let stationary_clip_count = self.clips.len() + 1;
        if let LayoutKind::Box { children, .. } = &mut self.kind {
            for child in children {
                if child.is_fixed_to_viewport() {
                    continue;
                }
                child.translate_scrolled_subtree(translation, stationary_clip_count);
            }
        }

        scroll.offset = offset;
        if let Some(scrollbar) = &mut scroll.horizontal {
            position_scrollbar_thumb(scrollbar, offset.x, scroll.max_offset.x);
        }
        if let Some(scrollbar) = &mut scroll.vertical {
            position_scrollbar_thumb(scrollbar, offset.y, scroll.max_offset.y);
        }
        true
    }

    pub(crate) fn is_fixed_to_viewport(&self) -> bool {
        matches!(
            self.kind,
            LayoutKind::Box {
                fixed_to_viewport: true,
                ..
            }
        )
    }

    fn translate_scrolled_subtree(
        &mut self,
        translation: ScrollOffset,
        stationary_clip_count: usize,
    ) {
        self.x += translation.x;
        self.y += translation.y;
        for clip in self.clips.iter_mut().skip(stationary_clip_count) {
            translate_rect(&mut clip.rect, translation);
        }

        if let Some(scroll) = &mut self.scroll {
            translate_rect(&mut scroll.viewport, translation);
            translate_rect(&mut scroll.clip.rect, translation);
            for scrollbar in [&mut scroll.horizontal, &mut scroll.vertical]
                .into_iter()
                .flatten()
            {
                translate_rect(&mut scrollbar.track, translation);
                translate_rect(&mut scrollbar.thumb, translation);
            }
        }

        if let LayoutKind::Box { children, .. } = &mut self.kind {
            for child in children {
                if child.is_fixed_to_viewport() {
                    continue;
                }
                child.translate_scrolled_subtree(translation, stationary_clip_count);
            }
        }
    }
}

fn position_scrollbar_thumb(scrollbar: &mut Scrollbar, offset: f32, max_offset: f32) {
    let (track_start, track_size, thumb_size) = match scrollbar.axis {
        ScrollbarAxis::Horizontal => (
            scrollbar.track.x,
            scrollbar.track.width,
            scrollbar.thumb.width,
        ),
        ScrollbarAxis::Vertical => (
            scrollbar.track.y,
            scrollbar.track.height,
            scrollbar.thumb.height,
        ),
    };
    let travel = (track_size - thumb_size).max(0.0);
    let position = if max_offset > 0.0 {
        travel * offset / max_offset
    } else {
        0.0
    };
    match scrollbar.axis {
        ScrollbarAxis::Horizontal => scrollbar.thumb.x = track_start + position,
        ScrollbarAxis::Vertical => scrollbar.thumb.y = track_start + position,
    }
}

fn translate_rect(rect: &mut Rect, translation: ScrollOffset) {
    rect.x += translation.x;
    rect.y += translation.y;
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

#[cfg(test)]
mod tests {
    use render::{BoxStyle, TextSpan, TextStyle};

    use crate::ui::elements::{ElementKind, BODY_ID};

    use super::*;

    fn text_layout(element_id: u64, x: f32, y: f32) -> Layout {
        Layout {
            element_id,
            x,
            y,
            width: 80.0,
            height: 20.0,
            transform: render::Transform::IDENTITY,
            clips: Vec::new(),
            scroll: None,
            kind: LayoutKind::Text {
                text: "Burokku".into(),
                spans: vec![TextSpan::new("Burokku", TextStyle::default())],
                style: TextStyle::default(),
                has_transform: false,
                line_count: 1,
                runs: Vec::new(),
            },
        }
    }

    fn box_layout(
        element_id: u64,
        z_index: Option<i32>,
        isolated: bool,
        children: Vec<Layout>,
    ) -> Layout {
        box_layout_with_position(element_id, z_index, isolated, Position::Relative, children)
    }

    fn box_layout_with_positioning(
        element_id: u64,
        z_index: Option<i32>,
        isolated: bool,
        positioned: bool,
        children: Vec<Layout>,
    ) -> Layout {
        let position = if positioned {
            Position::Relative
        } else {
            Position::Static
        };
        box_layout_with_position(element_id, z_index, isolated, position, children)
    }

    fn box_layout_with_position(
        element_id: u64,
        z_index: Option<i32>,
        isolated: bool,
        position: Position,
        children: Vec<Layout>,
    ) -> Layout {
        Layout {
            element_id,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            transform: render::Transform::IDENTITY,
            clips: Vec::new(),
            scroll: None,
            kind: LayoutKind::Box {
                style: BoxStyle::default(),
                has_transform: false,
                z_index,
                isolated,
                position,
                fixed_containing_block: None,
                fixed_to_viewport: false,
                flex_or_grid_item: false,
                children,
            },
        }
    }

    fn element_ids<'a>(layouts: impl Iterator<Item = &'a Layout>) -> Vec<u64> {
        layouts.map(Layout::element_id).collect()
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
            transform: render::Transform::IDENTITY,
            clips: Vec::new(),
            scroll: None,
            kind: LayoutKind::Box {
                style: BoxStyle::default(),
                has_transform: false,
                z_index: None,
                isolated: false,
                position: Position::Static,
                fixed_containing_block: None,
                fixed_to_viewport: false,
                flex_or_grid_item: false,
                children: vec![text_layout(2, 20.0, 20.0), text_layout(3, 20.0, 20.0)],
            },
        };

        assert_eq!(layout.hit_test(30.0, 30.0).unwrap().element_id(), 3);
        assert!(layout.hit_test(300.0, 300.0).is_none());
    }

    #[test]
    fn iterator_uses_stacking_context_render_order() {
        let high_descendant = box_layout(3, Some(10), false, vec![]);
        let ordinary_parent = box_layout(2, None, false, vec![high_descendant]);
        let middle_context = box_layout(4, Some(5), false, vec![]);
        let negative_context = box_layout(5, Some(-1), false, vec![]);
        let root = box_layout(
            1,
            None,
            false,
            vec![ordinary_parent, middle_context, negative_context],
        );

        assert_eq!(element_ids(root.iter()), [1, 5, 2, 4, 3]);
        assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 3);
    }

    #[test]
    fn isolation_contains_descendant_z_indices() {
        let high_descendant = box_layout(3, Some(10), false, vec![]);
        let isolated_parent = box_layout(2, None, true, vec![high_descendant]);
        let middle_context = box_layout(4, Some(5), false, vec![]);
        let root = box_layout(1, None, false, vec![isolated_parent, middle_context]);

        assert_eq!(element_ids(root.iter()), [1, 2, 3, 4]);
        assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 4);
    }

    #[test]
    fn reverse_iterator_is_the_exact_reverse_render_order() {
        let low = box_layout(2, Some(-2), false, vec![]);
        let automatic = box_layout(3, None, false, vec![text_layout(4, 0.0, 0.0)]);
        let high = box_layout(5, Some(8), false, vec![]);
        let root = box_layout(1, None, false, vec![high, automatic, low]);

        let forward = element_ids(root.iter());
        let reverse = element_ids(root.iter_rev());

        assert_eq!(forward, [1, 2, 3, 4, 5]);
        assert_eq!(reverse, [5, 4, 3, 2, 1]);
    }

    #[test]
    fn iterator_separates_block_decorations_from_text_content() {
        let first =
            box_layout_with_positioning(2, None, false, false, vec![text_layout(3, 20.0, 20.0)]);
        let second = box_layout_with_positioning(4, None, false, false, vec![]);
        let root = box_layout(1, None, false, vec![first, second]);

        assert_eq!(element_ids(root.iter()), [1, 2, 4, 3]);
        assert_eq!(element_ids(root.iter_rev()), [3, 4, 2, 1]);
        assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 3);
    }

    #[test]
    fn zero_level_contexts_paint_after_ordinary_content() {
        let zero = box_layout(2, Some(0), false, vec![]);
        let ordinary = box_layout_with_positioning(3, None, false, false, vec![]);
        let root = box_layout(1, None, false, vec![zero, ordinary]);

        assert_eq!(element_ids(root.iter()), [1, 3, 2]);
        assert_eq!(element_ids(root.iter_rev()), [2, 3, 1]);
        assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 2);
    }

    #[test]
    fn positioned_auto_paints_at_zero_level_without_containing_child_contexts() {
        let high_descendant = box_layout(3, Some(10), false, vec![]);
        let positioned_auto = box_layout(2, None, false, vec![high_descendant]);
        let ordinary = box_layout_with_positioning(4, None, false, false, vec![]);
        let middle_context = box_layout(5, Some(5), false, vec![]);
        let root = box_layout(
            1,
            None,
            false,
            vec![positioned_auto, ordinary, middle_context],
        );

        assert_eq!(element_ids(root.iter()), [1, 4, 2, 5, 3]);
        assert_eq!(element_ids(root.iter_rev()), [3, 5, 2, 4, 1]);
    }

    #[test]
    fn zero_z_index_contains_child_contexts_atomically() {
        let high_descendant = box_layout(3, Some(10), false, vec![]);
        let zero = box_layout(2, Some(0), false, vec![high_descendant]);
        let middle_context = box_layout(4, Some(5), false, vec![]);
        let root = box_layout(1, None, false, vec![zero, middle_context]);

        assert_eq!(element_ids(root.iter()), [1, 2, 3, 4]);
        assert_eq!(element_ids(root.iter_rev()), [4, 3, 2, 1]);
    }

    #[test]
    fn fixed_auto_contains_child_contexts_atomically() {
        let high_descendant = box_layout(3, Some(10), false, vec![]);
        let fixed =
            box_layout_with_position(2, None, false, Position::Fixed, vec![high_descendant]);
        let middle_context = box_layout(4, Some(5), false, vec![]);
        let root = box_layout(1, None, false, vec![fixed, middle_context]);

        assert_eq!(element_ids(root.iter()), [1, 2, 3, 4]);
        assert_eq!(element_ids(root.iter_rev()), [4, 3, 2, 1]);
        assert_eq!(root.hit_test(20.0, 20.0).unwrap().element_id(), 4);
    }

    #[test]
    fn effect_contexts_paint_after_ordinary_content_in_tree_order() {
        let mut document = Document::new();
        let isolated = document.create_node(ElementKind::Div);
        let translucent = document.create_node(ElementKind::Div);
        let transformed = document.create_node(ElementKind::Div);
        let ordinary = document.create_node(ElementKind::Div);

        document
            .set_style(isolated, "isolation", Some("isolate"))
            .unwrap();
        document
            .set_style(translucent, "opacity", Some("0.5"))
            .unwrap();
        document
            .set_style(transformed, "transform", Some("translateX(4px)"))
            .unwrap();
        for child in [isolated, translucent, transformed, ordinary] {
            document.insert(BODY_ID, child, None).unwrap();
        }

        let layout = compute_layout(&document, 100.0, 100.0, &mut TextSystem::new());

        assert_eq!(
            element_ids(layout.iter()),
            [BODY_ID, ordinary, isolated, translucent, transformed]
        );
        assert_eq!(
            element_ids(layout.iter_rev()),
            [transformed, translucent, isolated, ordinary, BODY_ID]
        );
    }
}
