//! Typed registration and ordered dispatch for backdrop renderers.
//!
//! A backdrop renderer samples the scene produced by preceding commands. Its
//! handle retains a device-independent factory, while each render engine lazily
//! creates a compatible target-specific renderer when it first sees the handle.

use std::{
    any::Any,
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crate::{
    canvas::DrawCommand,
    raster::{ClipMask, ClipMaskRange, ScissorRect},
};

static NEXT_RENDERER_ID: AtomicU64 = AtomicU64::new(1);

/// A strongly typed payload accepted by a backdrop renderer.
///
/// Payloads are retained behind an [`Arc`], so they do not need to implement
/// [`Clone`].
pub trait BackdropPayload: fmt::Debug + PartialEq + Send + Sync + 'static {}

impl<T> BackdropPayload for T where T: fmt::Debug + PartialEq + Send + Sync + 'static {}

/// GPU state available while a registered backdrop renderer creates its
/// pipeline.
#[derive(Clone, Copy, Debug)]
pub struct BackdropCreateContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
    /// Bind group layout reserved for the previous scene texture and sampler.
    ///
    /// Backdrop pipelines must install this layout at group `0`. Renderer-owned
    /// resources should begin at group `1`.
    pub scene_bind_group_layout: &'a wgpu::BindGroupLayout,
}

/// Per-frame state available while a registered renderer uploads its effects.
#[derive(Clone, Copy, Debug)]
pub struct BackdropPrepareContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub canvas_size: [u32; 2],
    /// Rounded masks resolved once from the frame's clip command stream.
    ///
    /// [`ResolvedBackdropDraw::clip_masks`] indexes this slice.
    pub clip_masks: &'a [ClipMask],
}

/// One renderer-local effect after central clip resolution.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedBackdropDraw<'a, P> {
    pub payload: &'a P,
    pub clip_masks: ClipMaskRange,
}

/// Constructs one target-specific renderer from a reusable registration.
pub trait BackdropRendererFactory<P: BackdropPayload>: Send + Sync + 'static {
    type Renderer: BackdropRenderer<P>;

    fn create(&self, context: BackdropCreateContext<'_>) -> Self::Renderer;
}

impl<P, R, F> BackdropRendererFactory<P> for F
where
    P: BackdropPayload,
    R: BackdropRenderer<P>,
    F: for<'a> Fn(BackdropCreateContext<'a>) -> R + Send + Sync + 'static,
{
    type Renderer = R;

    fn create(&self, context: BackdropCreateContext<'_>) -> Self::Renderer {
        self(context)
    }
}

/// A target-specific effect pipeline fed by typed, centrally resolved draws.
///
/// Before `draw` is called, the engine binds the previous scene texture and
/// sampler at bind group `0`, and applies the draw's rectangular scissor. A
/// renderer implements rounded clipping by consuming the mask range supplied
/// during `prepare`.
pub trait BackdropRenderer<P: BackdropPayload>: 'static {
    fn prepare(
        &mut self,
        context: BackdropPrepareContext<'_>,
        draws: &[ResolvedBackdropDraw<'_, P>],
    );

    fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, draw_index: usize);
}

/// A device-independent, reusable typed backdrop renderer registration.
///
/// The factory runs lazily once for each render target that encounters the
/// handle. Keep handles long-lived rather than constructing one every frame.
pub struct BackdropRendererHandle<P> {
    registration: Arc<RendererRegistration>,
    marker: PhantomData<fn(P) -> P>,
}

impl<P: BackdropPayload> BackdropRendererHandle<P> {
    pub fn new<F>(label: impl Into<Arc<str>>, factory: F) -> Self
    where
        F: BackdropRendererFactory<P>,
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
        DrawCommand::Backdrop(self.renderer_draw(payload))
    }

    pub(crate) fn renderer_draw(&self, payload: P) -> BackdropDraw {
        BackdropDraw {
            registration: Arc::clone(&self.registration),
            payload: Arc::new(payload),
        }
    }
}

impl<P> Clone for BackdropRendererHandle<P> {
    fn clone(&self) -> Self {
        Self {
            registration: Arc::clone(&self.registration),
            marker: PhantomData,
        }
    }
}

impl<P> fmt::Debug for BackdropRendererHandle<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackdropRendererHandle")
            .field("label", &self.registration.label)
            .field("id", &self.registration.id)
            .finish()
    }
}

impl<P> PartialEq for BackdropRendererHandle<P> {
    fn eq(&self, other: &Self) -> bool {
        self.registration.id == other.registration.id
    }
}

impl<P> Eq for BackdropRendererHandle<P> {}

impl<P> Hash for BackdropRendererHandle<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.registration.id.hash(state);
    }
}

/// An opaque retained payload created by [`BackdropRendererHandle::command`].
#[derive(Clone)]
pub struct BackdropDraw {
    registration: Arc<RendererRegistration>,
    payload: Arc<dyn ErasedPayload>,
}

impl fmt::Debug for BackdropDraw {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackdropDraw")
            .field("renderer", &self.registration.label)
            .field("payload", &self.payload)
            .finish()
    }
}

impl PartialEq for BackdropDraw {
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

impl<P: BackdropPayload> ErasedPayload for P {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn ErasedPayload) -> bool {
        other.as_any().downcast_ref::<P>() == Some(self)
    }
}

trait ErasedRendererFactory: Send + Sync {
    fn create(&self, context: BackdropCreateContext<'_>) -> Box<dyn ErasedBackdropRenderer>;
}

struct FactoryAdapter<P, F> {
    factory: F,
    marker: PhantomData<fn(P) -> P>,
}

impl<P, F> ErasedRendererFactory for FactoryAdapter<P, F>
where
    P: BackdropPayload,
    F: BackdropRendererFactory<P>,
{
    fn create(&self, context: BackdropCreateContext<'_>) -> Box<dyn ErasedBackdropRenderer> {
        Box::new(RendererAdapter::<P, F::Renderer> {
            renderer: self.factory.create(context),
            marker: PhantomData,
        })
    }
}

trait ErasedBackdropRenderer {
    fn prepare(&mut self, context: BackdropPrepareContext<'_>, draws: &[QueuedBackdropDraw]);

    fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, draw_index: usize);
}

struct RendererAdapter<P, R> {
    renderer: R,
    marker: PhantomData<fn(P) -> P>,
}

impl<P, R> ErasedBackdropRenderer for RendererAdapter<P, R>
where
    P: BackdropPayload,
    R: BackdropRenderer<P>,
{
    fn prepare(&mut self, context: BackdropPrepareContext<'_>, draws: &[QueuedBackdropDraw]) {
        let typed_draws = draws
            .iter()
            .map(|draw| ResolvedBackdropDraw {
                payload: draw
                    .payload
                    .as_any()
                    .downcast_ref::<P>()
                    .expect("renderer registration payload type must remain stable"),
                clip_masks: draw.clip_masks,
            })
            .collect::<Vec<_>>();
        self.renderer.prepare(context, &typed_draws);
    }

    fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, draw_index: usize) {
        self.renderer.draw(pass, draw_index);
    }
}

struct QueuedBackdropDraw {
    payload: Arc<dyn ErasedPayload>,
    clip_masks: ClipMaskRange,
}

struct RendererEntry {
    _registration: Arc<RendererRegistration>,
    renderer: Box<dyn ErasedBackdropRenderer>,
    draws: Vec<QueuedBackdropDraw>,
}

/// One backdrop invocation in the engine's global scene schedule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScheduledBackdrop {
    pub(crate) renderer_index: usize,
    pub(crate) draw_index: usize,
    pub(crate) scissor: ScissorRect,
}

/// Per-engine cache and frame queues for typed backdrop renderers.
pub(crate) struct BackdropSystem {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target_format: wgpu::TextureFormat,
    scene_bind_group_layout: wgpu::BindGroupLayout,
    entry_indices: HashMap<u64, usize>,
    entries: Vec<RendererEntry>,
}

impl BackdropSystem {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        scene_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            target_format,
            scene_bind_group_layout: scene_bind_group_layout.clone(),
            entry_indices: HashMap::new(),
            entries: Vec::new(),
        }
    }

    pub(crate) fn begin_frame(&mut self) {
        for entry in &mut self.entries {
            entry.draws.clear();
        }
    }

    pub(crate) fn queue(
        &mut self,
        draw: BackdropDraw,
        scissor: ScissorRect,
        clip_masks: ClipMaskRange,
    ) -> Option<ScheduledBackdrop> {
        if scissor.is_empty() {
            return None;
        }

        let renderer_index = self.renderer_index(&draw.registration);
        let draw_index = self.entries[renderer_index].draws.len();
        self.entries[renderer_index].draws.push(QueuedBackdropDraw {
            payload: draw.payload,
            clip_masks,
        });

        Some(ScheduledBackdrop {
            renderer_index,
            draw_index,
            scissor,
        })
    }

    pub(crate) fn prepare(&mut self, canvas_size: [u32; 2], clip_masks: &[ClipMask]) {
        let context = BackdropPrepareContext {
            device: &self.device,
            queue: &self.queue,
            canvas_size,
            clip_masks,
        };
        for entry in &mut self.entries {
            entry.renderer.prepare(context, &entry.draws);
        }
    }

    pub(crate) fn draw<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        scheduled: ScheduledBackdrop,
        source_bind_group: &'pass wgpu::BindGroup,
    ) {
        pass.set_scissor_rect(
            scheduled.scissor.x,
            scheduled.scissor.y,
            scheduled.scissor.width,
            scheduled.scissor.height,
        );
        pass.set_bind_group(0, source_bind_group, &[]);
        self.entries[scheduled.renderer_index]
            .renderer
            .draw(pass, scheduled.draw_index);
    }

    fn renderer_index(&mut self, registration: &Arc<RendererRegistration>) -> usize {
        if let Some(index) = self.entry_indices.get(&registration.id) {
            return *index;
        }

        let index = self.entries.len();
        let renderer = registration.factory.create(BackdropCreateContext {
            device: &self.device,
            queue: &self.queue,
            target_format: self.target_format,
            scene_bind_group_layout: &self.scene_bind_group_layout,
        });
        self.entries.push(RendererEntry {
            _registration: Arc::clone(registration),
            renderer,
            draws: Vec::new(),
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

    impl BackdropRendererFactory<TestPayload> for UnusedFactory {
        type Renderer = UnusedRenderer;

        fn create(&self, _context: BackdropCreateContext<'_>) -> Self::Renderer {
            UnusedRenderer
        }
    }

    impl BackdropRenderer<TestPayload> for UnusedRenderer {
        fn prepare(
            &mut self,
            _context: BackdropPrepareContext<'_>,
            _draws: &[ResolvedBackdropDraw<'_, TestPayload>],
        ) {
        }

        fn draw<'pass>(&'pass self, _pass: &mut wgpu::RenderPass<'pass>, _draw_index: usize) {}
    }

    #[test]
    fn retained_backdrop_draws_compare_by_registration_and_payload() {
        let first = BackdropRendererHandle::new("first", UnusedFactory);
        let same_registration = first.clone();
        let second = BackdropRendererHandle::new("second", UnusedFactory);

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
}
