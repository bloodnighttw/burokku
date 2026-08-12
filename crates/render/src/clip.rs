//! Central CPU-side clip resolution for retained draw commands.
//!
//! Rectangular clips become fixed-function scissor rectangles. Rounded clips
//! additionally become contiguous mask ranges that raster renderers can share
//! for every draw recorded in the same clip scope.

use bytemuck::{Pod, Zeroable};

use crate::shapes::{rect::Rect, round::Round};

/// One rounded-rectangle mask in the shared clip buffer.
///
/// The C layout maps to two consecutive WGSL `vec4<f32>` fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ClipMask {
    pub bounds: [f32; 4],
    pub round: [f32; 4],
}

impl ClipMask {
    fn new(rect: Rect, round: Round) -> Self {
        let round = round.fit(rect.width, rect.height);
        Self {
            bounds: [rect.x, rect.y, rect.width, rect.height],
            round: [round.lt, round.rt, round.rb, round.lb],
        }
    }

    fn is_rounded(self) -> bool {
        self.round.iter().any(|radius| *radius > 0.0)
    }
}

/// A contiguous range in the shared clip-mask buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
pub struct ClipMaskRange {
    pub start: u32,
    pub count: u32,
}

impl ClipMaskRange {
    pub const fn new(start: u32, count: u32) -> Self {
        Self { start, count }
    }

    pub const fn as_array(self) -> [u32; 2] {
        [self.start, self.count]
    }
}

/// A pixel-space scissor rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ScissorRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    fn intersect_rect(self, rect: Rect) -> Self {
        if rect.is_empty()
            || !rect.x.is_finite()
            || !rect.y.is_finite()
            || !rect.width.is_finite()
            || !rect.height.is_finite()
        {
            return Self::new(self.x, self.y, 0, 0);
        }

        let self_right = self.x.saturating_add(self.width);
        let self_bottom = self.y.saturating_add(self.height);
        let clip_left = float_edge(rect.x.floor(), self_right);
        let clip_top = float_edge(rect.y.floor(), self_bottom);
        let clip_right = float_edge((rect.x + rect.width).ceil(), self_right);
        let clip_bottom = float_edge((rect.y + rect.height).ceil(), self_bottom);

        let left = self.x.max(clip_left);
        let top = self.y.max(clip_top);
        let right = self_right.min(clip_right);
        let bottom = self_bottom.min(clip_bottom);
        Self::new(
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    }
}

/// The fully resolved clip applied to one draw command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedClip {
    pub(crate) scissor: ScissorRect,
    pub(crate) masks: ClipMaskRange,
}

/// A malformed clip command sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum ClipResolveError {
    #[error("clip commands contain an unmatched push or pop")]
    Unbalanced,
}

/// Resolves one command stream's nested clips into reusable frame data.
pub(crate) struct ClipResolver {
    current: ResolvedClip,
    ancestors: Vec<ResolvedClip>,
    masks: Vec<ClipMask>,
    unbalanced: bool,
}

impl ClipResolver {
    pub(crate) fn new(canvas_size: [u32; 2]) -> Self {
        Self {
            current: ResolvedClip {
                scissor: ScissorRect::new(0, 0, canvas_size[0], canvas_size[1]),
                masks: ClipMaskRange::default(),
            },
            ancestors: Vec::new(),
            masks: Vec::new(),
            unbalanced: false,
        }
    }

    pub(crate) const fn current(&self) -> ResolvedClip {
        self.current
    }

    pub(crate) fn push(&mut self, rect: Rect, round: Round) {
        let parent = self.current;
        self.ancestors.push(parent);

        let mask = ClipMask::new(rect, round);
        let masks = if mask.is_rounded() {
            let start = u32::try_from(self.masks.len())
                .expect("a frame cannot contain more than u32::MAX clip masks");
            let parent_start = parent.masks.start as usize;
            let parent_end = parent_start + parent.masks.count as usize;
            self.masks.extend_from_within(parent_start..parent_end);
            self.masks.push(mask);
            ClipMaskRange {
                start,
                count: parent
                    .masks
                    .count
                    .checked_add(1)
                    .expect("a clip scope cannot contain more than u32::MAX masks"),
            }
        } else {
            parent.masks
        };

        self.current = ResolvedClip {
            scissor: parent.scissor.intersect_rect(rect),
            masks,
        };
    }

    pub(crate) fn pop(&mut self) -> Result<(), ClipResolveError> {
        let Some(parent) = self.ancestors.pop() else {
            self.unbalanced = true;
            return Err(ClipResolveError::Unbalanced);
        };
        self.current = parent;
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<Vec<ClipMask>, ClipResolveError> {
        if self.unbalanced || !self.ancestors.is_empty() {
            return Err(ClipResolveError::Unbalanced);
        }
        Ok(self.masks)
    }
}

fn float_edge(value: f32, maximum: u32) -> u32 {
    value.clamp(0.0, maximum as f32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canvas::DrawList, offscreen::OffscreenSurface, shapes::rect::DrawRectExt};

    fn rounded(radius: f32) -> Round {
        Round {
            lt: radius,
            rt: radius,
            rb: radius,
            lb: radius,
        }
    }

    #[test]
    fn detects_clip_stack_underflow_and_unclosed_scopes() {
        let mut underflow = ClipResolver::new([100, 100]);
        assert_eq!(underflow.pop(), Err(ClipResolveError::Unbalanced));
        assert_eq!(underflow.finish(), Err(ClipResolveError::Unbalanced));

        let mut unclosed = ClipResolver::new([100, 100]);
        unclosed.push(Rect::new(0.0, 0.0, 10.0, 10.0), Round::default());
        assert_eq!(unclosed.finish(), Err(ClipResolveError::Unbalanced));

        let mut balanced = ClipResolver::new([100, 100]);
        balanced.push(Rect::new(0.0, 0.0, 10.0, 10.0), Round::default());
        assert_eq!(balanced.pop(), Ok(()));
        assert_eq!(balanced.finish(), Ok(Vec::new()));
    }

    #[test]
    fn nested_clips_intersect_and_restore() {
        let mut clips = ClipResolver::new([100, 100]);

        clips.push(Rect::new(10.0, 10.0, 50.0, 50.0), Round::default());
        assert_eq!(clips.current().scissor, ScissorRect::new(10, 10, 50, 50));

        clips.push(Rect::new(40.0, 0.0, 50.0, 30.0), Round::default());
        assert_eq!(clips.current().scissor, ScissorRect::new(40, 10, 20, 20));

        clips.pop().unwrap();
        assert_eq!(clips.current().scissor, ScissorRect::new(10, 10, 50, 50));
        clips.pop().unwrap();
        assert_eq!(clips.current().scissor, ScissorRect::new(0, 0, 100, 100));
        assert!(clips.finish().unwrap().is_empty());
    }

    #[test]
    fn rounded_scope_builds_its_mask_range_once_and_reuses_it() {
        let mut clips = ClipResolver::new([100, 100]);
        clips.push(Rect::new(10.0, 10.0, 80.0, 80.0), rounded(8.0));

        let first_draw_clip = clips.current();
        let second_draw_clip = clips.current();
        assert_eq!(first_draw_clip, second_draw_clip);
        assert_eq!(first_draw_clip.masks, ClipMaskRange { start: 0, count: 1 });
        assert_eq!(clips.masks.len(), 1);

        clips.push(Rect::new(20.0, 20.0, 60.0, 60.0), Round::default());
        assert_eq!(clips.current().masks, first_draw_clip.masks);
        assert_eq!(clips.masks.len(), 1);
        clips.pop().unwrap();
        clips.pop().unwrap();

        assert_eq!(clips.finish().unwrap().len(), 1);
    }

    #[test]
    fn nested_rounded_scope_copies_parent_into_one_contiguous_range() {
        let mut clips = ClipResolver::new([100, 100]);
        clips.push(Rect::new(0.0, 0.0, 80.0, 80.0), rounded(8.0));
        let parent = clips.current();
        clips.push(Rect::new(10.0, 10.0, 40.0, 40.0), rounded(4.0));
        let child = clips.current();

        assert_eq!(parent.masks, ClipMaskRange { start: 0, count: 1 });
        assert_eq!(child.masks, ClipMaskRange { start: 1, count: 2 });

        clips.pop().unwrap();
        assert_eq!(clips.current(), parent);
        clips.pop().unwrap();
        let masks = clips.finish().unwrap();
        assert_eq!(masks.len(), 3);
        assert_eq!(masks[0], masks[1]);
        assert_ne!(masks[1], masks[2]);
    }

    #[test]
    fn fractional_clip_edges_cover_partial_pixels() {
        let mut clips = ClipResolver::new([100, 100]);
        clips.push(Rect::new(10.25, 20.75, 5.5, 6.5), Round::default());

        assert_eq!(clips.current().scissor, ScissorRect::new(10, 20, 6, 8));
    }

    #[test]
    fn invalid_or_disjoint_clip_is_empty() {
        let mut invalid = ClipResolver::new([100, 100]);
        invalid.push(Rect::new(f32::NAN, 0.0, 10.0, 10.0), Round::default());
        assert!(invalid.current().scissor.is_empty());

        let mut disjoint = ClipResolver::new([100, 100]);
        disjoint.push(Rect::new(200.0, 200.0, 10.0, 10.0), Round::default());
        assert!(disjoint.current().scissor.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_pixels_match_nested_clip() {
        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping offscreen clip test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws.with_clip(Rect::new(2.0, 2.0, 12.0, 12.0), |draws| {
            draws.with_clip(Rect::new(4.0, 4.0, 8.0, 8.0), |draws| {
                draws.draw_rect(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED);
            });
        });

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 4, 4), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 11, 11), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 3, 4), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 12, 11), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 4, 3), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 11, 12), [0, 0, 255, 255]);
    }
}
