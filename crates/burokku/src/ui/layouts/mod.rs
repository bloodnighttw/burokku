//! Computed UI nodes ready for rendering and hit testing.
//! This should be used inside main threads.

mod compute;
mod iter;
mod stacking;
#[cfg(test)]
mod tests;

use render::{BoxStyle, Clip, Color, Rect, TextStyle, TextSystem};
use std::collections::HashMap;

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
        stacking_layer: StackingLayer,
        native_appearance: Option<NativeAppearance>,
        children: Vec<Layout>,
    },
    Text {
        text: String,
        style: TextStyle,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeAppearance {
    Button,
    Select { color: Color },
}

impl Layout {
    pub fn element_id(&self) -> u64 {
        self.element_id
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.clips.iter().all(|clip| clip.contains(x, y))
            && self.width > 0.0
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
