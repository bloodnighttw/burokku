//! Native resize subscriptions backed by the existing DOM and completed layouts.
//!
//! Layout computation publishes snapshots. The host delivers native callbacks
//! later; consumers can also explicitly take records. No JavaScript is required.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    task::Waker,
};

use slotmap::{new_key_type, SlotMap};
use taffy::geometry::Size;
use thiserror::Error;

use super::{
    elements::{Dom, DomError, NodeId},
    layout::ComputedLayout,
};

new_key_type! { struct ObserverId; }

// Currently observes only the layout's width and height.
// TODO: add content-box/border-box selection when the style API supports the
// required border and CSS box-sizing behavior.

/// An owned measurement in logical pixels. `target` belongs to the source DOM.
/// Keeping an entry does not retain its target after the subscription is removed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResizeObserverEntry {
    pub target: NodeId,
    pub size: Size<f32>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ResizeObserverError {
    #[error("the observer is closed")]
    Closed,
    #[error(transparent)]
    InvalidTarget(#[from] DomError),
}

#[derive(Debug)]
struct Observation {
    target: NodeId,
    generation: u64,
    last: Option<Size<f32>>,
}

type Callback = Rc<RefCell<dyn FnMut(&[ResizeObserverEntry])>>;

#[derive(Default)]
struct ObserverState {
    observations: Vec<Observation>,
    callback: Option<Callback>,
}

impl std::fmt::Debug for ObserverState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObserverState")
            .field("observations", &self.observations)
            .field("has_callback", &self.callback.is_some())
            .finish()
    }
}

#[derive(Debug, Default)]
struct RegistryState {
    observers: SlotMap<ObserverId, ObserverState>,
    generation: u64,
    latest: Option<Rc<ComputedLayout>>,
    waker: Option<Waker>,
    measurement_requested: bool,
    delivery_pending: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ResizeObserverRegistry(Rc<RefCell<RegistryState>>);

impl ResizeObserverRegistry {
    pub(crate) fn create(&self) -> ResizeObserver {
        let id = self
            .0
            .borrow_mut()
            .observers
            .insert(ObserverState::default());
        ResizeObserver {
            registry: Rc::downgrade(&self.0),
            id,
        }
    }

    /// Called only after a successful layout, including valid cache hits.
    pub(crate) fn publish(&self, layout: Rc<ComputedLayout>) {
        let wake = {
            let mut state = self.0.borrow_mut();
            let changed = state
                .latest
                .as_ref()
                .is_none_or(|old| !Rc::ptr_eq(old, &layout));
            let requested = std::mem::take(&mut state.measurement_requested);
            state.latest = Some(layout);
            if (changed || requested)
                && !state.delivery_pending
                && state
                    .observers
                    .values()
                    .any(|o| o.callback.is_some() && !o.observations.is_empty())
            {
                state.delivery_pending = true;
                state.waker.clone()
            } else {
                None
            }
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }

    pub(crate) fn set_waker(&self, waker: Waker) {
        let pending = {
            let mut state = self.0.borrow_mut();
            state.waker = Some(waker.clone());
            state.measurement_requested || state.delivery_pending
        };
        if pending {
            waker.wake();
        }
    }

    pub(crate) fn request_measurement(&self) {
        let wake = {
            let mut state = self.0.borrow_mut();
            if state.measurement_requested
                || !state.observers.values().any(|o| !o.observations.is_empty())
            {
                return;
            }
            state.measurement_requested = true;
            state.waker.clone()
        };
        if let Some(waker) = wake {
            waker.wake();
        }
    }

    pub(crate) fn measurement_requested(&self) -> bool {
        self.0.borrow().measurement_requested
    }

    pub(crate) fn measurement_failed(&self) {
        let mut state = self.0.borrow_mut();
        state.measurement_requested = false;
        state.delivery_pending = false;
    }

    /// Captures registration generations before any callback gets to mutate them.
    pub(crate) fn prepare_callbacks(&self, dom: &Dom) -> Vec<NativeDelivery> {
        let mut state = self.0.borrow_mut();
        if !std::mem::take(&mut state.delivery_pending) {
            return Vec::new();
        }
        state
            .observers
            .iter()
            .filter_map(|(id, observer)| {
                let callback = observer.callback.as_ref()?.clone();
                let batch = prepare_batch(&state, id, dom).ok()??;
                Some(NativeDelivery { callback, batch })
            })
            .collect()
    }

    pub(crate) fn shutdown(&self) {
        // Drop captured application values after releasing the registry borrow.
        let observers = {
            let mut state = self.0.borrow_mut();
            state.waker = None;
            state.latest = None;
            state.measurement_requested = false;
            state.delivery_pending = false;
            state
                .observers
                .drain()
                .map(|(_, observer)| observer)
                .collect::<Vec<_>>()
        };
        drop(observers);
    }

    pub(crate) fn retained_targets(&self) -> Vec<NodeId> {
        self.0
            .borrow()
            .observers
            .values()
            .flat_map(|observer| observer.observations.iter().map(|o| o.target))
            .collect()
    }

    pub(crate) fn prune(&self, dom: &Dom) {
        for observer in self.0.borrow_mut().observers.values_mut() {
            observer.observations.retain(|o| dom.contains(o.target));
        }
    }
}

/// An owned subscription, created by [`Dom::resize_observer`].
///
/// Targets are existing `NodeId`s from the application's single DOM arena.
/// Dropping this handle cancels registrations and releases their retention roots.
#[derive(Debug)]
pub struct ResizeObserver {
    registry: Weak<RefCell<RegistryState>>,
    id: ObserverId,
}

impl Drop for ResizeObserver {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            let removed = registry.borrow_mut().observers.remove(self.id);
            drop(removed);
        }
    }
}

impl ResizeObserver {
    /// Sets a native callback, invoked by the host after successful measurement.
    /// The callback runs with no DOM or observer-registry borrow held. Captures
    /// may be UI-thread `Rc` values; `Send` is not required.
    ///
    /// Keep the returned observer alive for as long as it should watch the node:
    ///
    /// ```
    /// use burokku::ui::{
    ///     elements::{Dom, NodeId},
    ///     resize_observer::{ResizeObserver, ResizeObserverError},
    /// };
    ///
    /// fn watch_window(dom: &Dom, window: NodeId) -> Result<ResizeObserver, ResizeObserverError> {
    ///     let observer = dom.resize_observer();
    ///     observer.set_callback(|entries| {
    ///         for entry in entries {
    ///             println!("{:?}: {} x {}", entry.target, entry.size.width, entry.size.height);
    ///         }
    ///     })?;
    ///     observer.observe(dom, window)?;
    ///     Ok(observer)
    /// }
    /// ```
    pub fn set_callback<F>(&self, callback: F) -> Result<(), ResizeObserverError>
    where
        F: FnMut(&[ResizeObserverEntry]) + 'static,
    {
        let registry = self.registry()?;
        let old = {
            let mut state = registry.borrow_mut();
            let observer = state
                .observers
                .get_mut(self.id)
                .ok_or(ResizeObserverError::Closed)?;
            observer.callback.replace(Rc::new(RefCell::new(callback)))
        };
        drop(old);
        ResizeObserverRegistry(registry).request_measurement();
        Ok(())
    }

    /// Registers an element, or requests a fresh initial size if already registered.
    pub fn observe(&self, dom: &Dom, target: NodeId) -> Result<(), ResizeObserverError> {
        let registry = self.registry()?;
        dom.element_tag(target)?;
        let mut state = registry.borrow_mut();
        state.generation = state
            .generation
            .checked_add(1)
            .expect("resize registration IDs exhausted");
        let generation = state.generation;
        let observer = state
            .observers
            .get_mut(self.id)
            .ok_or(ResizeObserverError::Closed)?;
        observer.observations.retain(|o| o.target != target);
        observer.observations.push(Observation {
            target,
            generation,
            last: None,
        });
        drop(state);
        ResizeObserverRegistry(registry).request_measurement();
        Ok(())
    }

    /// Removes a registration. Missing targets and closed documents are no-ops.
    pub fn unobserve(&self, target: NodeId) {
        if let Some(registry) = self.registry.upgrade() {
            if let Some(observer) = registry.borrow_mut().observers.get_mut(self.id) {
                observer.observations.retain(|o| o.target != target);
            }
        }
    }

    /// Removes all registrations; the observer can be reused while its DOM lives.
    pub fn disconnect(&self) {
        if let Some(registry) = self.registry.upgrade() {
            if let Some(observer) = registry.borrow_mut().observers.get_mut(self.id) {
                observer.observations.clear();
            }
        }
    }

    /// Takes changed measurements from the latest successful layout.
    ///
    /// Returns no entries before layout, or when the DOM changed since that
    /// layout. This never computes layout or calls user code. Consuming records
    /// advances only this observer's last-reported sizes.
    pub fn take_records(&self, dom: &Dom) -> Result<Vec<ResizeObserverEntry>, ResizeObserverError> {
        match self.prepare(dom)? {
            Some(batch) => self.commit(dom, batch),
            None => Ok(Vec::new()),
        }
    }

    fn registry(&self) -> Result<Rc<RefCell<RegistryState>>, ResizeObserverError> {
        self.registry.upgrade().ok_or(ResizeObserverError::Closed)
    }

    // Adapters prepare entries before committing, so failed conversion does not
    // consume a delivery. No registry borrow survives either method.
    pub(crate) fn prepare(&self, dom: &Dom) -> Result<Option<ResizeBatch>, ResizeObserverError> {
        let registry = self.registry()?;
        let state = registry.borrow();
        prepare_batch(&state, self.id, dom)
    }

    /// A stale batch is discarded as a whole; valid registrations remain pending.
    pub(crate) fn commit(
        &self,
        dom: &Dom,
        batch: ResizeBatch,
    ) -> Result<Vec<ResizeObserverEntry>, ResizeObserverError> {
        let registry = self.registry()?;
        if batch.observer != self.id {
            return Ok(Vec::new());
        }
        let mut state = registry.borrow_mut();
        commit_batch(&mut state, dom, batch)
    }
}

/// Prepares changed entries for one observer by comparing the latest completed
/// layout sizes with its last-reported sizes. Retains the layout snapshot and
/// registration generations so commit_batch can reject stale work after an
/// earlier callback disconnects or re-observes a target.
/// This does not compute layout, invoke callbacks, or advance reported sizes;
/// only a successful commit consumes the batch.
fn prepare_batch(
    state: &RegistryState,
    id: ObserverId,
    dom: &Dom,
) -> Result<Option<ResizeBatch>, ResizeObserverError> {
    let observations = &state
        .observers
        .get(id)
        .ok_or(ResizeObserverError::Closed)?
        .observations;
    let Some(layout) = state.latest.as_ref() else {
        return Ok(None);
    };
    if layout.revision() != dom.revision() {
        return Ok(None);
    }
    let mut entries = Vec::new();
    let mut registrations = Vec::new();
    for observation in observations {
        let Ok(connected) = dom.is_connected(observation.target) else {
            continue;
        };
        if !connected && observation.last.is_none() {
            continue;
        }
        let entry = measure(layout, observation.target, connected);
        let size = entry.size;
        if observation.last == Some(size) {
            continue;
        }
        entries.push(entry);
        registrations.push((observation.target, observation.generation, size));
    }
    if entries.is_empty() {
        return Ok(None);
    }
    Ok(Some(ResizeBatch {
        observer: id,
        entries,
        registrations,
        layout: layout.clone(),
    }))
}

fn commit_batch(
    state: &mut RegistryState,
    dom: &Dom,
    batch: ResizeBatch,
) -> Result<Vec<ResizeObserverEntry>, ResizeObserverError> {
    if batch.layout.revision() != dom.revision()
        || !state
            .latest
            .as_ref()
            .is_some_and(|latest| Rc::ptr_eq(latest, &batch.layout))
    {
        return Ok(Vec::new());
    }
    let observations = &mut state
        .observers
        .get_mut(batch.observer)
        .ok_or(ResizeObserverError::Closed)?
        .observations;
    if !batch
        .registrations
        .iter()
        .all(|(target, generation, size)| {
            dom.contains(*target)
                && observations.iter().any(|o| {
                    o.target == *target && o.generation == *generation && o.last != Some(*size)
                })
        })
    {
        return Ok(Vec::new());
    }
    for (target, generation, size) in batch.registrations {
        let observation = observations
            .iter_mut()
            .find(|o| o.target == target && o.generation == generation)
            .expect("batch registration was validated");
        observation.last = Some(size);
    }
    Ok(batch.entries)
}

pub(crate) struct NativeDelivery {
    callback: Callback,
    batch: ResizeBatch,
}

impl NativeDelivery {
    /// Commits under a short DOM borrow; invoke the returned closure afterwards.
    pub(crate) fn commit(self, dom: &Dom) -> Option<impl FnOnce()> {
        let entries = {
            let mut state = dom.resize_observers.0.borrow_mut();
            let observer = state.observers.get(self.batch.observer)?;
            if !observer
                .callback
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, &self.callback))
            {
                return None;
            }
            commit_batch(&mut state, dom, self.batch).ok()?
        };
        if entries.is_empty() {
            return None;
        }
        let callback = self.callback;
        Some(move || (callback.borrow_mut())(&entries))
    }
}

pub(crate) struct ResizeBatch {
    observer: ObserverId,
    pub(crate) entries: Vec<ResizeObserverEntry>,
    registrations: Vec<(NodeId, u64, Size<f32>)>,
    layout: Rc<ComputedLayout>,
}

fn canonical(value: f32) -> f32 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn measure(layout: &ComputedLayout, target: NodeId, connected: bool) -> ResizeObserverEntry {
    let Some(computed) = connected.then(|| layout.box_for(target)).flatten() else {
        return ResizeObserverEntry {
            target,
            ..Default::default()
        };
    };
    ResizeObserverEntry {
        target,
        size: computed.layout().size.map(canonical),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        task::Wake,
    };

    use super::*;
    use crate::ui::{
        elements::ElementTag,
        layout::{
            LayoutEngine, LogicalViewport, TextMeasureRequest, TextMeasurement, TextMeasurer,
        },
        text::TextEngine,
    };

    fn fixture(width: &str) -> (Dom, NodeId, NodeId) {
        let mut dom = Dom::new();
        let window = dom.create_element_tag(ElementTag::Window);
        let panel = dom.create_element_tag(ElementTag::Div);
        dom.set_style_property(panel, "width", width).unwrap();
        dom.set_style_property(panel, "height", "50px").unwrap();
        dom.append_child(window, panel).unwrap();
        dom.append_child(dom.root(), window).unwrap();
        (dom, window, panel)
    }

    fn layout(dom: &Dom, width: f32, height: f32) {
        LayoutEngine::new(TextEngine::without_system_fonts())
            .compute(dom, LogicalViewport::new(width, height).unwrap())
            .unwrap();
    }

    #[derive(Default)]
    struct WakeCount(AtomicUsize);

    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn observer_and_dom_changes_coalesce_wakeups_and_idle_callbacks_reuse_layout() {
        let (mut dom, _, panel) = fixture("100px");
        let wakes = Arc::new(WakeCount::default());
        dom.resize_observers.set_waker(Waker::from(wakes.clone()));
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        observer.observe(&dom, panel).unwrap();
        dom.set_style_property(panel, "width", "120px").unwrap();
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        let mut engine = LayoutEngine::new(TextEngine::without_system_fonts());
        let viewport = LogicalViewport::new(320.0, 240.0).unwrap();
        engine.compute(&dom, viewport).unwrap();
        assert!(!dom.resize_observers.measurement_requested());
        // A manual subscription has no callback delivery to wake for.
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        dom.set_style_property(panel, "width", "140px").unwrap();
        assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
        engine.compute(&dom, viewport).unwrap();
        let cached = engine.current_shared().unwrap();
        let calls = Rc::new(Cell::new(0));
        let recorded = calls.clone();
        observer
            .set_callback(move |entries| {
                assert_eq!(entries[0].size.width, 140.0);
                recorded.set(recorded.get() + 1);
            })
            .unwrap();
        assert_eq!(wakes.0.load(Ordering::SeqCst), 3);
        engine.compute(&dom, viewport).unwrap();
        assert!(Rc::ptr_eq(&cached, &engine.current_shared().unwrap()));
        assert_eq!(wakes.0.load(Ordering::SeqCst), 4);
        for delivery in dom.resize_observers.prepare_callbacks(&dom) {
            delivery.commit(&dom).unwrap()();
        }
        assert_eq!(calls.get(), 1);
        engine.compute(&dom, viewport).unwrap();
        assert!(dom.resize_observers.prepare_callbacks(&dom).is_empty());
        assert_eq!(wakes.0.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn initial_entries_batch_and_observing_after_idle_layout_works() {
        let (mut dom, window, panel) = fixture("100px");
        let zero = dom.create_element_tag(ElementTag::Div);
        dom.set_style_property(zero, "width", "0px").unwrap();
        dom.set_style_property(zero, "height", "0px").unwrap();
        dom.append_child(window, zero).unwrap();
        layout(&dom, 320.0, 240.0);
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        observer.observe(&dom, zero).unwrap();
        let entries = observer.take_records(&dom).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].size.width, 100.0);
        assert_eq!(entries[1].size, Size::ZERO);
        assert!(observer.take_records(&dom).unwrap().is_empty());
        layout(&dom, 320.0, 240.0);
        assert!(observer.take_records(&dom).unwrap().is_empty());
    }

    #[test]
    fn padding_and_position_changes_without_size_changes_stay_quiet() {
        let (mut dom, _, panel) = fixture("100px");
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        layout(&dom, 320.0, 240.0);
        let entry = observer.take_records(&dom).unwrap()[0];
        assert_eq!(
            entry.size,
            Size {
                width: 100.0,
                height: 50.0
            }
        );
        dom.set_style_property(panel, "padding", "10px").unwrap();
        layout(&dom, 320.0, 240.0);
        assert!(observer.take_records(&dom).unwrap().is_empty());
        dom.set_style_property(panel, "margin", "5px").unwrap();
        layout(&dom, 320.0, 240.0);
        assert!(observer.take_records(&dom).unwrap().is_empty());
        dom.set_style_property(panel, "width", "120px").unwrap();
        layout(&dom, 320.0, 240.0);
        assert_eq!(
            observer.take_records(&dom).unwrap()[0].size,
            Size {
                width: 120.0,
                height: 50.0
            }
        );
    }

    #[test]
    fn viewport_changes_resize_percent_children_but_not_fixed_children() {
        let (mut dom, window, fixed) = fixture("100px");
        let percent = dom.create_element_tag(ElementTag::Div);
        dom.set_style_property(percent, "width", "50%").unwrap();
        dom.append_child(window, percent).unwrap();
        let observer = dom.resize_observer();
        for target in [window, fixed, percent] {
            observer.observe(&dom, target).unwrap();
        }
        layout(&dom, 320.0, 240.0);
        observer.take_records(&dom).unwrap();
        layout(&dom, 400.0, 240.0);
        let entries = observer.take_records(&dom).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.target).collect::<Vec<_>>(),
            [window, percent]
        );
        assert_eq!(entries[1].size.width, 200.0);
        layout(&dom, 0.0, 0.0);
        let entries = observer.take_records(&dom).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].size, Size::ZERO);
        assert_eq!(entries[1].size.width, 0.0);
    }

    #[test]
    fn intermediate_layouts_coalesce_and_a_stale_dom_waits_for_layout() {
        let (mut dom, _, panel) = fixture("100px");
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        assert!(observer.take_records(&dom).unwrap().is_empty());
        layout(&dom, 320.0, 240.0);
        observer.take_records(&dom).unwrap();
        for width in ["120px", "150px"] {
            dom.set_style_property(panel, "width", width).unwrap();
            layout(&dom, 320.0, 240.0);
        }
        dom.set_style_property(panel, "width", "180px").unwrap();
        assert!(observer.take_records(&dom).unwrap().is_empty());
        layout(&dom, 320.0, 240.0);
        let entries = observer.take_records(&dom).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size.width, 180.0);
    }

    #[test]
    fn detached_targets_wait_then_transition_to_zero_and_reattach() {
        let (mut dom, window, panel) = fixture("100px");
        dom.detach(panel).unwrap();
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        layout(&dom, 320.0, 240.0);
        assert!(observer.take_records(&dom).unwrap().is_empty());
        assert!(dom
            .reclaim_unreachable_detached([])
            .unwrap()
            .nodes
            .is_empty());
        dom.append_child(window, panel).unwrap();
        layout(&dom, 320.0, 240.0);
        assert_eq!(observer.take_records(&dom).unwrap()[0].size.width, 100.0);
        dom.detach(panel).unwrap();
        layout(&dom, 320.0, 240.0);
        assert_eq!(observer.take_records(&dom).unwrap()[0].size, Size::ZERO);
        assert!(observer.take_records(&dom).unwrap().is_empty());
        dom.append_child(window, panel).unwrap();
        layout(&dom, 320.0, 240.0);
        assert_eq!(observer.take_records(&dom).unwrap()[0].size.width, 100.0);
    }

    #[test]
    fn cancelled_and_replaced_batches_do_not_consume_new_registrations() {
        let (dom, _, panel) = fixture("100px");
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        layout(&dom, 320.0, 240.0);
        // Discarding a prepared batch models adapter entry-conversion failure.
        drop(observer.prepare(&dom).unwrap().unwrap());
        let old = observer.prepare(&dom).unwrap().unwrap();
        observer.observe(&dom, panel).unwrap();
        assert!(observer.commit(&dom, old).unwrap().is_empty());
        assert_eq!(observer.take_records(&dom).unwrap().len(), 1);
        observer.observe(&dom, panel).unwrap();
        let pending = observer.prepare(&dom).unwrap().unwrap();
        observer.disconnect();
        observer.disconnect();
        assert!(observer.commit(&dom, pending).unwrap().is_empty());
        observer.observe(&dom, panel).unwrap();
        assert_eq!(observer.take_records(&dom).unwrap().len(), 1);
        observer.unobserve(panel);
        observer.unobserve(panel);
        assert!(observer.take_records(&dom).unwrap().is_empty());
    }

    #[test]
    fn dropping_subscriptions_releases_detached_components_independently() {
        let (mut dom, _, panel) = fixture("100px");
        dom.detach(panel).unwrap();
        let first = dom.resize_observer();
        let second = dom.resize_observer();
        for observer in [&first, &second] {
            observer.observe(&dom, panel).unwrap();
        }
        drop(first);
        assert!(dom
            .reclaim_unreachable_detached([])
            .unwrap()
            .nodes
            .is_empty());
        drop(second);
        assert_eq!(dom.reclaim_unreachable_detached([]).unwrap().nodes, [panel]);
    }

    #[test]
    fn explicit_destruction_removes_roots_and_invalidates_pending_entries() {
        let (mut dom, _, panel) = fixture("100px");
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        layout(&dom, 320.0, 240.0);
        let batch = observer.prepare(&dom).unwrap().unwrap();
        dom.remove_subtree(panel).unwrap();
        assert!(observer.commit(&dom, batch).unwrap().is_empty());
        assert!(dom.resize_observers.retained_targets().is_empty());
        assert!(dom.reclaim_unreachable_detached([]).is_ok());
        assert!(matches!(
            observer.observe(&dom, panel),
            Err(ResizeObserverError::InvalidTarget(_))
        ));
    }

    #[test]
    fn rejects_non_elements_and_closes_with_dom() {
        let (mut dom, _, _) = fixture("100px");
        let observer = dom.resize_observer();
        let text = dom.create_text("raw text");
        for invalid in [dom.root(), text] {
            assert!(matches!(
                observer.observe(&dom, invalid),
                Err(ResizeObserverError::InvalidTarget(
                    DomError::NodeNotElement(_)
                ))
            ));
        }
        drop(dom);
        assert!(matches!(
            observer.registry(),
            Err(ResizeObserverError::Closed)
        ));
        observer.disconnect();
    }

    #[derive(Default)]
    struct ChangingGeneration {
        generation: Cell<u64>,
        fail: bool,
    }

    impl TextMeasurer for ChangingGeneration {
        fn generation(&self) -> u64 {
            let generation = self.generation.get();
            if self.fail {
                self.generation.set(generation + 1);
            }
            generation
        }
        fn measure(&mut self, _: TextMeasureRequest<'_>) -> Result<TextMeasurement, String> {
            Err("the fixture should not contain text".into())
        }
    }

    #[test]
    fn failed_layout_preserves_delivery_state_until_a_successful_retry() {
        let (mut dom, _, panel) = fixture("100px");
        let observer = dom.resize_observer();
        observer.observe(&dom, panel).unwrap();
        let mut engine = LayoutEngine::new(ChangingGeneration::default());
        let viewport = LogicalViewport::new(320.0, 240.0).unwrap();
        engine.compute(&dom, viewport).unwrap();
        observer.take_records(&dom).unwrap();
        dom.set_style_property(panel, "width", "150px").unwrap();
        engine.measurer_mut().fail = true;
        assert!(engine.compute(&dom, viewport).is_err());
        assert!(observer.take_records(&dom).unwrap().is_empty());
        engine.measurer_mut().fail = false;
        engine.compute(&dom, viewport).unwrap();
        assert_eq!(observer.take_records(&dom).unwrap()[0].size.width, 150.0);
        assert!(observer.take_records(&dom).unwrap().is_empty());
    }
}
