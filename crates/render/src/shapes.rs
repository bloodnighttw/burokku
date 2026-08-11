pub mod rect;
pub mod round;
mod rounded_rect;
pub mod stroke;

use crate::{canvas::DrawCommand, shapes::rounded_rect::RoundedRectRenderer};

pub(crate) struct ShapeRenderer {
    renderers: Vec<Box<dyn ShapePipeline>>,
    batches: Vec<PreparedBatch>,
}

/// The common lifecycle for one persistent GPU shape pipeline.
///
/// Implementations own their pipelines, buffers, and renderer-local batches.
/// [`ShapeRenderer`] combines those local batches into one command-ordered
/// schedule and calls back into the owning renderer when encoding a frame.
trait ShapePipeline {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: &[DrawCommand],
        canvas_size: [u32; 2],
    );

    fn batch_count(&self) -> usize;

    fn batch_order(&self, batch_index: usize) -> usize;

    fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, batch_index: usize);
}

/// A batch contributed by one primitive renderer, keyed by its first command.
///
/// Primitive renderers keep their own GPU buffers and local batches. This
/// schedule is the shared layer that restores command-list order when those
/// batches come from different render pipelines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreparedBatch {
    order: usize,
    renderer_index: usize,
    batch_index: usize,
}

impl ShapeRenderer {
    pub(crate) fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        Self {
            // in future, we can expand it with custom wgsl shaders.
            renderers: vec![Box::new(RoundedRectRenderer::new(device, surface_format))],
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
        self.batches.clear();
        for (renderer_index, renderer) in self.renderers.iter_mut().enumerate() {
            renderer.prepare(device, queue, commands, canvas_size);
            self.batches.extend(
                (0..renderer.batch_count()).map(|batch_index| PreparedBatch {
                    order: renderer.batch_order(batch_index),
                    renderer_index,
                    batch_index,
                }),
            );
        }
        self.batches.sort_unstable_by_key(|batch| batch.order);
    }

    pub(crate) fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        for batch in &self.batches {
            self.renderers[batch.renderer_index].draw_batch(pass, batch.batch_index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_batches_are_sorted_by_original_command_order() {
        let mut batches = [
            PreparedBatch {
                order: 4,
                renderer_index: 0,
                batch_index: 2,
            },
            PreparedBatch {
                order: 0,
                renderer_index: 0,
                batch_index: 0,
            },
            PreparedBatch {
                order: 2,
                renderer_index: 1,
                batch_index: 0,
            },
        ];

        batches.sort_unstable_by_key(|batch| batch.order);

        assert_eq!(batches.map(|batch| batch.order), [0, 2, 4],);
        assert_eq!(
            batches.map(|batch| (batch.renderer_index, batch.batch_index)),
            [(0, 0), (1, 0), (0, 2)],
        );
    }
}
