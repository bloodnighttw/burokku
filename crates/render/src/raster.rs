//! Typed registration and ordered dispatch for raster renderers.
//!
//! A handle contains a device-independent renderer factory. Recording a draw
//! retains that registration and a typed payload; each render target lazily
//! creates its own compatible GPU renderer when it first sees the handle.

use std::{
    any::Any,
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    ops::Range,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crate::canvas::DrawCommand;

static NEXT_RENDERER_ID: AtomicU64 = AtomicU64::new(1);
pub use crate::clip::{ClipMask, ClipMaskRange, ScissorRect};

/// A strongly typed payload accepted by a raster renderer.
///
/// Payloads are retained behind an `Arc`, so they do not need to implement
/// [`Clone`].
pub trait RasterPayload: fmt::Debug + PartialEq + Send + Sync + 'static {}

impl<T> RasterPayload for T where T: fmt::Debug + PartialEq + Send + Sync + 'static {}

/// GPU state available while a registered renderer creates its pipeline.
#[derive(Clone, Copy, Debug)]
pub struct RasterCreateContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
    pub sample_count: u32,
}

/// Per-frame state available while a registered renderer uploads its draws.
#[derive(Clone, Copy, Debug)]
pub struct RasterPrepareContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub canvas_size: [u32; 2],
    /// Rounded masks resolved once from the frame's clip command stream.
    ///
    /// [`ResolvedRasterDraw::clip_masks`] indexes this slice. Shaders that
    /// support rounded clipping should test every mask in that range.
    pub clip_masks: &'a [ClipMask],
}

/// One renderer-local draw after central clip resolution.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedRasterDraw<'a, P> {
    pub payload: &'a P,
    pub clip_masks: ClipMaskRange,
}

/// One maximal adjacent run for a renderer under one scissor rectangle.
///
/// The engine applies `scissor` immediately before calling `draw_batch`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterBatch {
    pub scissor: ScissorRect,
    pub draws: Range<usize>,
}

/// Constructs one target-specific renderer from a reusable registration.
pub trait RasterRendererFactory<P: RasterPayload>: Send + Sync + 'static {
    type Renderer: RasterRenderer<P>;

    fn create(&self, context: RasterCreateContext<'_>) -> Self::Renderer;
}

impl<P, R, F> RasterRendererFactory<P> for F
where
    P: RasterPayload,
    R: RasterRenderer<P>,
    F: for<'a> Fn(RasterCreateContext<'a>) -> R + Send + Sync + 'static,
{
    type Renderer = R;

    fn create(&self, context: RasterCreateContext<'_>) -> Self::Renderer {
        self(context)
    }
}

/// A target-specific raster pipeline fed by typed, centrally resolved draws.
///
/// `batches` is ordered and has a stable one-to-one relationship with
/// `draw_batch` indices. A renderer may cull draws, but it must retain an empty
/// slot for a batch whose draws were all culled. Rectangular clipping is
/// enforced by the engine's scissor state; renderers implement rounded clips
/// by consuming each draw's mask range during `prepare`.
pub trait RasterRenderer<P: RasterPayload>: 'static {
    fn prepare(
        &mut self,
        context: RasterPrepareContext<'_>,
        draws: &[ResolvedRasterDraw<'_, P>],
        batches: &[RasterBatch],
    );

    fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, batch_index: usize);
}

/// A device-independent, reusable typed renderer registration.
///
/// The factory runs lazily once for each render target that encounters the
/// handle. Keep handles long-lived rather than constructing one every frame.
pub struct RasterRendererHandle<P> {
    registration: Arc<RendererRegistration>,
    marker: PhantomData<fn(P) -> P>,
}

impl<P: RasterPayload> RasterRendererHandle<P> {
    pub fn new<F>(label: impl Into<Arc<str>>, factory: F) -> Self
    where
        F: RasterRendererFactory<P>,
    {
        Self {
            registration: Arc::new(RendererRegistration {
                id: NEXT_RENDERER_ID.fetch_add(1, Ordering::Relaxed),
                label: label.into(),
                factory: Box::new(FactoryAdapter::<P, F> {
                    factory,
                    marker: PhantomData,
                }),
            }),
            marker: PhantomData,
        }
    }

    /// Creates a retained command for this renderer and payload.
    pub fn command(&self, payload: P) -> DrawCommand {
        DrawCommand::Raster(self.renderer_draw(payload))
    }

    pub(crate) fn renderer_draw(&self, payload: P) -> RendererDraw {
        RendererDraw {
            registration: Arc::clone(&self.registration),
            payload: Arc::new(payload),
        }
    }
}

impl<P> Clone for RasterRendererHandle<P> {
    fn clone(&self) -> Self {
        Self {
            registration: Arc::clone(&self.registration),
            marker: PhantomData,
        }
    }
}

impl<P> fmt::Debug for RasterRendererHandle<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RasterRendererHandle")
            .field("label", &self.registration.label)
            .field("id", &self.registration.id)
            .finish()
    }
}

impl<P> PartialEq for RasterRendererHandle<P> {
    fn eq(&self, other: &Self) -> bool {
        self.registration.id == other.registration.id
    }
}

impl<P> Eq for RasterRendererHandle<P> {}

impl<P> Hash for RasterRendererHandle<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.registration.id.hash(state);
    }
}

/// An opaque retained payload created by [`RasterRendererHandle::command`].
#[derive(Clone)]
pub struct RendererDraw {
    registration: Arc<RendererRegistration>,
    payload: Arc<dyn ErasedPayload>,
}

impl fmt::Debug for RendererDraw {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RendererDraw")
            .field("renderer", &self.registration.label)
            .field("payload", &self.payload)
            .finish()
    }
}

impl PartialEq for RendererDraw {
    fn eq(&self, other: &Self) -> bool {
        self.registration.id == other.registration.id && self.payload.equals(other.payload.as_ref())
    }
}

struct RendererRegistration {
    id: u64,
    label: Arc<str>,
    factory: Box<dyn ErasedRendererFactory>,
}

trait ErasedPayload: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn equals(&self, other: &dyn ErasedPayload) -> bool;
}

impl<P: RasterPayload> ErasedPayload for P {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn ErasedPayload) -> bool {
        other.as_any().downcast_ref::<P>() == Some(self)
    }
}

trait ErasedRendererFactory: Send + Sync {
    fn create(&self, context: RasterCreateContext<'_>) -> Box<dyn ErasedRasterRenderer>;
}

struct FactoryAdapter<P, F> {
    factory: F,
    marker: PhantomData<fn(P) -> P>,
}

impl<P, F> ErasedRendererFactory for FactoryAdapter<P, F>
where
    P: RasterPayload,
    F: RasterRendererFactory<P>,
{
    fn create(&self, context: RasterCreateContext<'_>) -> Box<dyn ErasedRasterRenderer> {
        Box::new(RendererAdapter::<P, F::Renderer> {
            renderer: self.factory.create(context),
            marker: PhantomData,
        })
    }
}

trait ErasedRasterRenderer {
    fn prepare(
        &mut self,
        context: RasterPrepareContext<'_>,
        draws: &[QueuedRasterDraw],
        batches: &[RasterBatch],
    );

    fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, batch_index: usize);
}

struct RendererAdapter<P, R> {
    renderer: R,
    marker: PhantomData<fn(P) -> P>,
}

impl<P, R> ErasedRasterRenderer for RendererAdapter<P, R>
where
    P: RasterPayload,
    R: RasterRenderer<P>,
{
    fn prepare(
        &mut self,
        context: RasterPrepareContext<'_>,
        draws: &[QueuedRasterDraw],
        batches: &[RasterBatch],
    ) {
        let typed_draws = draws
            .iter()
            .map(|draw| ResolvedRasterDraw {
                payload: draw
                    .payload
                    .as_any()
                    .downcast_ref::<P>()
                    .expect("renderer registration payload type must remain stable"),
                clip_masks: draw.clip_masks,
            })
            .collect::<Vec<_>>();
        self.renderer.prepare(context, &typed_draws, batches);
    }

    fn draw_batch<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, batch_index: usize) {
        self.renderer.draw_batch(pass, batch_index);
    }
}

struct QueuedRasterDraw {
    payload: Arc<dyn ErasedPayload>,
    clip_masks: ClipMaskRange,
}

struct RendererEntry {
    _registration: Arc<RendererRegistration>,
    renderer: Box<dyn ErasedRasterRenderer>,
    draws: Vec<QueuedRasterDraw>,
    batches: Vec<RasterBatch>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScheduledBatch {
    renderer_index: usize,
    batch_index: usize,
    scissor: ScissorRect,
}

/// Per-engine cache and frame schedule for typed raster renderers.
pub(crate) struct RasterSystem {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    entry_indices: HashMap<u64, usize>,
    entries: Vec<RendererEntry>,
    schedule: Vec<ScheduledBatch>,
    can_extend_last_batch: bool,
}

impl RasterSystem {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            target_format,
            sample_count,
            entry_indices: HashMap::new(),
            entries: Vec::new(),
            schedule: Vec::new(),
            can_extend_last_batch: false,
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        self.schedule.clear();
        self.can_extend_last_batch = false;
        for entry in &mut self.entries {
            entry.draws.clear();
            entry.batches.clear();
        }
    }

    pub(crate) fn queue(
        &mut self,
        draw: RendererDraw,
        scissor: ScissorRect,
        clip_masks: ClipMaskRange,
    ) -> Option<usize> {
        if scissor.is_empty() {
            return None;
        }

        let renderer_index = self.renderer_index(&draw.registration);
        let draw_index = self.entries[renderer_index].draws.len();
        self.entries[renderer_index].draws.push(QueuedRasterDraw {
            payload: draw.payload,
            clip_masks,
        });

        if self.can_extend_last_batch {
            if let Some(scheduled) = self.schedule.last() {
                if scheduled.renderer_index == renderer_index && scheduled.scissor == scissor {
                    self.entries[renderer_index].batches[scheduled.batch_index]
                        .draws
                        .end = draw_index + 1;
                    return Some(self.schedule.len() - 1);
                }
            }
        }

        let batch_index = self.entries[renderer_index].batches.len();
        self.entries[renderer_index].batches.push(RasterBatch {
            scissor,
            draws: draw_index..draw_index + 1,
        });
        self.schedule.push(ScheduledBatch {
            renderer_index,
            batch_index,
            scissor,
        });
        self.can_extend_last_batch = true;
        Some(self.schedule.len() - 1)
    }

    /// Prevents a later raster draw from joining a batch across a compositor
    /// operation such as a backdrop effect.
    pub(crate) fn break_batch(&mut self) {
        self.can_extend_last_batch = false;
    }

    pub(crate) fn prepare(&mut self, canvas_size: [u32; 2], clip_masks: &[ClipMask]) {
        let context = RasterPrepareContext {
            device: &self.device,
            queue: &self.queue,
            canvas_size,
            clip_masks,
        };
        for entry in &mut self.entries {
            entry
                .renderer
                .prepare(context, &entry.draws, &entry.batches);
        }
    }

    pub(crate) fn draw_range<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        batches: Range<usize>,
    ) {
        for scheduled in &self.schedule[batches] {
            pass.set_scissor_rect(
                scheduled.scissor.x,
                scheduled.scissor.y,
                scheduled.scissor.width,
                scheduled.scissor.height,
            );
            self.entries[scheduled.renderer_index]
                .renderer
                .draw_batch(pass, scheduled.batch_index);
        }
    }

    fn renderer_index(&mut self, registration: &Arc<RendererRegistration>) -> usize {
        if let Some(index) = self.entry_indices.get(&registration.id) {
            return *index;
        }

        let index = self.entries.len();
        let renderer = registration.factory.create(RasterCreateContext {
            device: &self.device,
            queue: &self.queue,
            target_format: self.target_format,
            sample_count: self.sample_count,
        });
        self.entries.push(RendererEntry {
            _registration: Arc::clone(registration),
            renderer,
            draws: Vec::new(),
            batches: Vec::new(),
        });
        self.entry_indices.insert(registration.id, index);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestPayload(u32);

    struct UnusedFactory;
    struct UnusedRenderer;

    impl RasterRendererFactory<TestPayload> for UnusedFactory {
        type Renderer = UnusedRenderer;

        fn create(&self, _context: RasterCreateContext<'_>) -> Self::Renderer {
            UnusedRenderer
        }
    }

    impl RasterRenderer<TestPayload> for UnusedRenderer {
        fn prepare(
            &mut self,
            _context: RasterPrepareContext<'_>,
            _draws: &[ResolvedRasterDraw<'_, TestPayload>],
            _batches: &[RasterBatch],
        ) {
        }

        fn draw_batch<'pass>(
            &'pass self,
            _pass: &mut wgpu::RenderPass<'pass>,
            _batch_index: usize,
        ) {
        }
    }

    #[test]
    fn retained_renderer_draws_compare_by_registration_and_payload() {
        let first = RasterRendererHandle::new("first", UnusedFactory);
        let same_registration = first.clone();
        let second = RasterRendererHandle::new("second", UnusedFactory);

        assert_eq!(
            first.command(TestPayload(7)),
            same_registration.command(TestPayload(7))
        );
        assert_ne!(first.command(TestPayload(7)), first.command(TestPayload(8)));
        assert_ne!(
            first.command(TestPayload(7)),
            second.command(TestPayload(7))
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn typed_registration_preserves_interleaved_renderer_order() {
        use crate::{
            canvas::DrawList,
            offscreen::OffscreenSurface,
            shapes::{
                rect::{DrawRectExt, Rect},
                round::Round,
                rounded_rect::{RoundedRectDraw, RoundedRectRendererFactory},
            },
        };

        let Some(mut surface) = OffscreenSurface::new([16, 16]).await else {
            eprintln!("skipping typed renderer test: no WebGPU adapter available");
            return;
        };
        let custom = RasterRendererHandle::new(
            "custom rounded rectangle",
            |context: RasterCreateContext<'_>| RoundedRectRendererFactory.create(context),
        );
        let mut draws = DrawList::new();
        draws
            .draw_rect(Rect::new(0.0, 0.0, 16.0, 16.0), wgpu::Color::RED)
            .draw_with(
                &custom,
                RoundedRectDraw::Fill {
                    rect: Rect::new(2.0, 2.0, 12.0, 12.0),
                    color: wgpu::Color::BLUE,
                    round: Round::default(),
                },
            )
            .draw_rect(Rect::new(4.0, 4.0, 8.0, 8.0), wgpu::Color::GREEN)
            .draw_with(
                &custom,
                RoundedRectDraw::Fill {
                    rect: Rect::new(6.0, 6.0, 4.0, 4.0),
                    color: wgpu::Color::WHITE,
                    round: Round::default(),
                },
            );

        let pixels = surface.render_rgba8(&draws, wgpu::Color::BLACK).await;

        assert_eq!(surface.pixel(&pixels, 1, 1), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 3, 3), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&pixels, 5, 5), [0, 255, 0, 255]);
        assert_eq!(surface.pixel(&pixels, 7, 7), [255, 255, 255, 255]);

        let mut clipped = DrawList::new();
        clipped.with_rounded_clip(
            Rect::new(2.0, 2.0, 12.0, 12.0),
            Round {
                lt: 4.0,
                rt: 4.0,
                rb: 4.0,
                lb: 4.0,
            },
            |draws| {
                draws.draw_with(
                    &custom,
                    RoundedRectDraw::Fill {
                        rect: Rect::new(0.0, 0.0, 16.0, 16.0),
                        color: wgpu::Color::RED,
                        round: Round::default(),
                    },
                );
            },
        );

        let clipped_pixels = surface.render_rgba8(&clipped, wgpu::Color::BLUE).await;

        assert_eq!(surface.pixel(&clipped_pixels, 2, 2), [0, 0, 255, 255]);
        assert_eq!(surface.pixel(&clipped_pixels, 8, 2), [255, 0, 0, 255]);
        assert_eq!(surface.pixel(&clipped_pixels, 8, 8), [255, 0, 0, 255]);
    }
}
