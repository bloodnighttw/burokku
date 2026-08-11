//! CPU-side rectangular clipping for retained draw commands.
//!
//! Rectangular clips use wgpu's fixed-function scissor state. Rounded clips
//! keep that scissor as a coarse bound and add a per-fragment mask in the
//! primitive renderers.

use bytemuck::{Pod, Zeroable};

use crate::{
    canvas::DrawCommand,
    shapes::{rect::Rect, round::Round},
};

pub(crate) fn commands_are_balanced(commands: &[DrawCommand]) -> bool {
    let mut depth = 0usize;
    for command in commands {
        match command {
            DrawCommand::PushClip { .. } => depth += 1,
            DrawCommand::PopClip if depth == 0 => return false,
            DrawCommand::PopClip => depth -= 1,
            DrawCommand::Rect { .. } => {}
            DrawCommand::Stroke { .. } => {}
        }
    }
    depth == 0
}

pub(crate) struct ClipStack {
    active: ScissorRect,
    ancestors: Vec<ScissorRect>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ClipMask {
    bounds: [f32; 4],
    round: [f32; 4],
}

impl ClipMask {
    pub(crate) fn new(rect: Rect, round: Round) -> Self {
        let round = round.fit(rect.width, rect.height);
        Self {
            bounds: [rect.x, rect.y, rect.width, rect.height],
            round: [round.lt, round.rt, round.rb, round.lb],
        }
    }

    pub(crate) fn is_rounded(self) -> bool {
        self.round.iter().any(|radius| *radius > 0.0)
    }
}

impl ClipStack {
    pub(crate) fn new(canvas_size: [u32; 2]) -> Self {
        Self {
            active: ScissorRect::new(0, 0, canvas_size[0], canvas_size[1]),
            ancestors: Vec::new(),
        }
    }

    pub(crate) fn active(&self) -> ScissorRect {
        self.active
    }

    pub(crate) fn push(&mut self, rect: Rect) {
        self.ancestors.push(self.active);
        self.active = self.active.intersect_rect(rect);
    }

    pub(crate) fn pop(&mut self) {
        self.active = self
            .ancestors
            .pop()
            .expect("clip commands are validated before rectangle preparation");
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScissorRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl ScissorRect {
    pub(crate) const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) const fn is_empty(self) -> bool {
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

fn float_edge(value: f32, maximum: u32) -> u32 {
    value.clamp(0.0, maximum as f32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canvas::DrawList,
        offscreen::OffscreenSurface,
        shapes::{rect::DrawRectExt, round::Round},
    };

    #[test]
    fn validates_balanced_and_unbalanced_command_lists() {
        let clip = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(commands_are_balanced(&[
            DrawCommand::push_clip(clip, Round::default()),
            DrawCommand::pop_clip(),
        ]));
        assert!(!commands_are_balanced(&[DrawCommand::pop_clip()]));
        assert!(!commands_are_balanced(&[DrawCommand::push_clip(
            clip,
            Round::default()
        )]));
    }

    #[test]
    fn nested_clips_intersect_and_restore() {
        let mut clips = ClipStack::new([100, 100]);

        clips.push(Rect::new(10.0, 10.0, 50.0, 50.0));
        assert_eq!(clips.active(), ScissorRect::new(10, 10, 50, 50));

        clips.push(Rect::new(40.0, 0.0, 50.0, 30.0));
        assert_eq!(clips.active(), ScissorRect::new(40, 10, 20, 20));

        clips.pop();
        assert_eq!(clips.active(), ScissorRect::new(10, 10, 50, 50));
        clips.pop();
        assert_eq!(clips.active(), ScissorRect::new(0, 0, 100, 100));
    }

    #[test]
    fn fractional_clip_edges_cover_partial_pixels() {
        let mut clips = ClipStack::new([100, 100]);

        clips.push(Rect::new(10.25, 20.75, 5.5, 6.5));

        assert_eq!(clips.active(), ScissorRect::new(10, 20, 6, 8));
    }

    #[test]
    fn invalid_or_disjoint_clip_is_empty() {
        let mut invalid = ClipStack::new([100, 100]);
        invalid.push(Rect::new(f32::NAN, 0.0, 10.0, 10.0));
        assert!(invalid.active().is_empty());

        let mut disjoint = ClipStack::new([100, 100]);
        disjoint.push(Rect::new(200.0, 200.0, 10.0, 10.0));
        assert!(disjoint.active().is_empty());
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
