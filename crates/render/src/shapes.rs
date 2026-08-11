pub mod rect;
pub mod round;
pub mod stroke;

use crate::{
    canvas::DrawCommand,
    shapes::{rect::RectRenderer, stroke::StrokeRenderer},
};

pub(crate) struct ShapeRenderer {
    rect: RectRenderer,
    stroke: StrokeRenderer,
    batches: Vec<ShapeBatch>,
}

impl ShapeRenderer {
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        Self {
            rect: RectRenderer::new(device, surface_format),
            stroke: StrokeRenderer::new(device, surface_format),
            batches: Vec::new(),
        }
    }

    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: &[DrawCommand],
        canvas_size: [u32; 2],
    ) {
        self.rect.prepare(device, queue, commands, canvas_size);
        self.stroke.prepare(device, queue, commands, canvas_size);

        self.batches.clear();
        let mut rect = 0;
        let mut stroke = 0;
        while rect < self.rect.batch_count() || stroke < self.stroke.batch_count() {
            let take_rect = match (
                rect < self.rect.batch_count(),
                stroke < self.stroke.batch_count(),
            ) {
                (true, false) => true,
                (false, true) => false,
                (true, true) => self.rect.batch_order(rect) < self.stroke.batch_order(stroke),
                (false, false) => break,
            };

            if take_rect {
                self.batches.push(ShapeBatch::Rect(rect));
                rect += 1;
            } else {
                self.batches.push(ShapeBatch::Stroke(stroke));
                stroke += 1;
            }
        }
    }

    pub(crate) fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        for batch in &self.batches {
            match *batch {
                ShapeBatch::Rect(index) => self.rect.draw_batch(pass, index),
                ShapeBatch::Stroke(index) => self.stroke.draw_batch(pass, index),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShapeBatch {
    Rect(usize),
    Stroke(usize),
}
