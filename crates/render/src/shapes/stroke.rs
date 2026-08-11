//! Rectangular-outline geometry and retained drawing helpers.

use crate::{
    canvas::{DrawCommand, DrawList},
    shapes::{rect::Rect, round::Round},
};

/// An inward-facing outline around rectangular bounds.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Stroke {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub line_width: f32,
}

impl Stroke {
    pub const fn new(x: f32, y: f32, width: f32, height: f32, line_width: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            line_width,
        }
    }

    pub const fn from_rect(rect: Rect, line_width: f32) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height, line_width)
    }

    pub const fn rect(self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }

    pub fn is_empty(self) -> bool {
        self.rect().is_empty() || self.line_width <= 0.0 || !self.line_width.is_finite()
    }
}

/// Convenience stroke recording for real frames and mock [`DrawList`]s.
pub trait DrawStrokeExt {
    fn draw_stroke(&mut self, stroke: Stroke, color: wgpu::Color) -> &mut Self;
    fn draw_rounded_stroke(
        &mut self,
        stroke: Stroke,
        color: wgpu::Color,
        round: Round,
    ) -> &mut Self;
}

impl DrawStrokeExt for DrawList {
    fn draw_stroke(&mut self, stroke: Stroke, color: wgpu::Color) -> &mut Self {
        self.draw_rounded_stroke(stroke, color, Round::default())
    }

    fn draw_rounded_stroke(
        &mut self,
        stroke: Stroke,
        color: wgpu::Color,
        round: Round,
    ) -> &mut Self {
        self.draw(DrawCommand::stroke(stroke, color, round))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_positive_or_invalid_strokes_are_empty() {
        assert!(Stroke::new(0.0, 0.0, 0.0, 10.0, 1.0).is_empty());
        assert!(Stroke::new(0.0, 0.0, 10.0, 10.0, 0.0).is_empty());
        assert!(Stroke::new(0.0, 0.0, 10.0, 10.0, f32::NAN).is_empty());
        assert!(Stroke::new(f32::NAN, 0.0, 10.0, 10.0, 1.0).is_empty());
        assert!(!Stroke::new(0.0, 0.0, 10.0, 20.0, 2.0).is_empty());
    }

    #[test]
    fn extension_records_a_rounded_stroke_without_a_gpu() {
        let mut draws = DrawList::new();
        let stroke = Stroke::new(1.0, 2.0, 30.0, 40.0, 3.0);
        let round = Round {
            lt: 1.0,
            rt: 2.0,
            rb: 3.0,
            lb: 4.0,
        };

        draws.draw_rounded_stroke(stroke, wgpu::Color::GREEN, round);

        assert_eq!(
            draws.commands(),
            &[DrawCommand::Stroke {
                stroke,
                round,
                color: wgpu::Color::GREEN,
            }]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn offscreen_pixels_form_a_rounded_outline() {
        let Some(mut surface) = crate::offscreen::OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping offscreen stroke test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws.draw_rounded_stroke(
            Stroke::new(2.0, 2.0, 12.0, 12.0, 2.0),
            wgpu::Color::RED,
            Round {
                lt: 4.0,
                rt: 4.0,
                rb: 4.0,
                lb: 4.0,
            },
        );

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 8, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 3), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 4), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 8, 8), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 2, 2), [0, 0, 255, 255]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn rounded_clip_masks_a_stroke() {
        let Some(mut surface) = crate::offscreen::OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping offscreen clipped stroke test: no WebGPU adapter available");
            return;
        };
        let mut draws = DrawList::new();
        draws.with_rounded_clip(
            Rect::new(2.0, 2.0, 12.0, 12.0),
            Round {
                lt: 4.0,
                rt: 4.0,
                rb: 4.0,
                lb: 4.0,
            },
            |draws| {
                draws.draw_stroke(Stroke::new(0.0, 0.0, 16.0, 16.0, 8.0), wgpu::Color::RED);
            },
        );

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&pixels, 8, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 2, 2), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 1, 8), [0, 0, 255, 255]);
    }
}
