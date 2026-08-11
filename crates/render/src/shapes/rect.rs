//! Rectangle geometry and retained drawing helpers.

use crate::{
    canvas::{DrawCommand, DrawList},
    shapes::round::Round,
};

/// A rectangle in logical canvas pixels.
///
/// Coordinates start at the canvas's top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0.0
            || self.height <= 0.0
            || !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
    }
}

/// Convenience rectangle recording for real frames and mock [`DrawList`]s.
pub trait DrawRectExt {
    fn draw_rect(&mut self, rect: Rect, color: wgpu::Color) -> &mut Self;
    fn draw_rounded_rect(&mut self, rect: Rect, color: wgpu::Color, round: Round) -> &mut Self;
}

impl DrawRectExt for DrawList {
    fn draw_rect(&mut self, rect: Rect, color: wgpu::Color) -> &mut Self {
        self.draw_rounded_rect(rect, color, Round::default())
    }

    fn draw_rounded_rect(&mut self, rect: Rect, color: wgpu::Color, round: Round) -> &mut Self {
        self.draw(DrawCommand::rect(rect, color, round))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_positive_or_invalid_rectangles_are_empty() {
        assert!(Rect::new(0.0, 0.0, 0.0, 10.0).is_empty());
        assert!(Rect::new(0.0, 0.0, 10.0, -1.0).is_empty());
        assert!(Rect::new(f32::NAN, 0.0, 10.0, 10.0).is_empty());
        assert!(Rect::new(0.0, 0.0, f32::INFINITY, 10.0).is_empty());
        assert!(!Rect::new(0.0, 0.0, 10.0, 20.0).is_empty());
    }

    #[test]
    fn extension_records_a_rectangle_without_a_gpu() {
        let mut draws = DrawList::new();
        let rect = Rect::new(1.0, 2.0, 3.0, 4.0);

        draws.draw_rect(rect, wgpu::Color::GREEN);

        assert_eq!(
            draws.commands(),
            &[DrawCommand::Rect {
                rect,
                color: wgpu::Color::GREEN,
                round: Round::default(),
            }]
        );
    }

    #[test]
    fn extension_records_a_rounded_rectangle_without_a_gpu() {
        let mut draws = DrawList::new();
        let rect = Rect::new(1.0, 2.0, 30.0, 40.0);
        let round = Round {
            lt: 1.0,
            rt: 2.0,
            rb: 3.0,
            lb: 4.0,
        };

        draws.draw_rounded_rect(rect, wgpu::Color::GREEN, round);

        assert_eq!(
            draws.commands(),
            &[DrawCommand::Rect {
                rect,
                color: wgpu::Color::GREEN,
                round,
            }]
        );
    }
}
