//! JavaScript adapter over the native DOM resize observer.
//!
//! Installation is crate-owned. Runtime-backed contexts receive coalesced delivery
//! tasks; no JavaScript values are captured in the native signal or task queue.

use std::{cell::Cell, rc::Rc};

use runtime::{JsTaskQueue, JsTaskQueueError};

use rquickjs::{
    class::Trace,
    object::Property,
    prelude::{Opt, This},
    Array, CatchResultExt, Class, Ctx, Exception, Function, JsLifetime, Object, Result, Value,
};

use super::{borrow, errors, node::NativeNode, SharedDomBindings};
use crate::ui::{
    elements::NodeId,
    resize_observer::{ResizeBatch, ResizeObserver, ResizeObserverEntry, ResizeObserverError},
};

const REGISTRY: &str = "__burokkuResizeObservers";

/// One pending task or capacity waiter per adapter. This contains no JS values.
struct DeliverySignal {
    queue: JsTaskQueue,
    pending: Cell<bool>,
    dirty: Cell<bool>,
    closed: Cell<bool>,
}

impl DeliverySignal {
    fn new(queue: JsTaskQueue) -> Rc<Self> {
        Rc::new(Self {
            queue,
            pending: Cell::new(false),
            dirty: Cell::new(false),
            closed: Cell::new(false),
        })
    }

    fn close(&self) {
        self.closed.set(true);
        self.pending.set(false);
        self.dirty.set(false);
    }

    fn schedule(self: &Rc<Self>, allow_retry: bool) {
        if self.closed.get() || self.pending.replace(true) {
            return;
        }
        match self
            .queue
            .try_enqueue(move |ctx| Self::run(ctx, allow_retry))
        {
            Ok(()) => {}
            Err(JsTaskQueueError::Closed) => self.close(),
            Err(JsTaskQueueError::Full) => {
                // Keep pending set while waiting, so further layouts only update
                // the native snapshot. The waiter stays on the UI thread; only
                // the retry flag is captured by the Send queue closure.
                let queue = self.queue.clone();
                let signal = Rc::downgrade(self);
                tokio::task::spawn_local(async move {
                    if queue
                        .enqueue(move |ctx| Self::run(ctx, allow_retry))
                        .await
                        .is_err()
                    {
                        if let Some(signal) = signal.upgrade() {
                            signal.close();
                        }
                    }
                });
            }
        }
    }

    fn run(ctx: &Ctx<'_>, allow_retry: bool) -> Result<()> {
        // Resolve context-owned state here instead of capturing an Rc in the
        // queue's Send closure. All state access stays on the runtime/UI thread.
        let signal = registry(ctx)?.borrow().signal.clone();
        let Some(signal) = signal else {
            return Ok(());
        };
        if signal.closed.get() {
            return Ok(());
        }
        signal.dirty.set(false);
        let failed = if let Err(error) = deliver(ctx).catch(ctx) {
            eprintln!("Burokku ResizeObserver delivery failed: {error}");
            true
        } else {
            false
        };
        signal.pending.set(false);
        let dirty = signal.dirty.replace(false);
        // Do not lose a layout published during a callback. A conversion failure
        // gets one immediate retry; persistent failures wait for another signal
        // rather than spinning. Native records remain unconsumed on failure.
        if dirty || (failed && allow_retry) {
            signal.schedule(dirty);
        }
        Ok(())
    }

    fn request(self: &Rc<Self>) {
        if self.closed.get() {
            return;
        }
        self.dirty.set(true);
        self.schedule(true);
    }
}

#[derive(Trace, JsLifetime)]
struct Target<'js> {
    #[qjs(skip_trace)]
    id: NodeId,
    wrapper: Object<'js>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class(rename = "ResizeObserver")]
struct JsResizeObserver<'js> {
    #[qjs(skip_trace)]
    native: ResizeObserver,
    callback: Function<'js>,
    targets: Vec<Target<'js>>,
}

#[derive(Trace, JsLifetime)]
#[rquickjs::class]
struct ObserverRegistry<'js> {
    #[qjs(skip_trace)]
    dom: SharedDomBindings,
    #[qjs(skip_trace)]
    signal: Option<Rc<DeliverySignal>>,
    active: Vec<Class<'js, JsResizeObserver<'js>>>,
    freeze: Function<'js>,
}

impl Drop for ObserverRegistry<'_> {
    fn drop(&mut self) {
        if let Some(signal) = &self.signal {
            signal.close();
        }
    }
}

fn registry<'js>(ctx: &Ctx<'js>) -> Result<Class<'js, ObserverRegistry<'js>>> {
    Class::<JsResizeObserver>::prototype(ctx)?
        .expect("resize observer prototype exists")
        .get(REGISTRY)
}

fn target_id(ctx: &Ctx<'_>, target: &Object<'_>, dom: &SharedDomBindings) -> Result<NodeId> {
    let invalid = || Exception::throw_type(ctx, "ResizeObserver requires an element target");
    let node = Class::<NativeNode>::from_object(target).ok_or_else(invalid)?;
    let id = node.borrow().id;
    borrow(ctx, dom)?
        .dom
        .element_tag(id)
        .map_err(|_| invalid())?;
    Ok(id)
}

fn map_native<T>(ctx: &Ctx<'_>, result: std::result::Result<T, ResizeObserverError>) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(ResizeObserverError::Closed) => {
            errors::throw_named(ctx, "InvalidStateError", "the resize observer is closed")
        }
        Err(error) => Err(Exception::throw_type(ctx, &error.to_string())),
    }
}

#[rquickjs::methods]
impl<'js> JsResizeObserver<'js> {
    #[qjs(constructor)]
    fn new(ctx: Ctx<'js>, callback: Function<'js>) -> Result<Self> {
        let (dom, signal) = {
            let registry = registry(&ctx)?;
            let registry = registry.borrow();
            (registry.dom.clone(), registry.signal.clone())
        };
        let native = borrow(&ctx, &dom)?.dom.resize_observer();
        if let Some(signal) = signal {
            let signal = Rc::downgrade(&signal);
            map_native(
                &ctx,
                native.set_delivery_notify(move || {
                    if let Some(signal) = signal.upgrade() {
                        signal.request();
                    }
                }),
            )?;
        }
        Ok(Self {
            native,
            callback,
            targets: Vec::new(),
        })
    }

    fn observe(
        this: This<Class<'js, Self>>,
        ctx: Ctx<'js>,
        target: Object<'js>,
        options: Opt<Value<'js>>,
    ) -> Result<()> {
        // Only layout width/height is supported. Do not read user option getters.
        if options.0.is_some_and(|value| !value.is_undefined()) {
            return Err(Exception::throw_type(
                &ctx,
                "ResizeObserver options are not supported yet",
            ));
        }
        let registry = registry(&ctx)?;
        let dom = registry.borrow().dom.clone();
        let id = target_id(&ctx, &target, &dom)?;
        let activate = {
            let state = borrow(&ctx, &dom)?;
            let mut observer = this.0.borrow_mut();
            map_native(&ctx, observer.native.observe(&state.dom, id))?;
            let activate = observer.targets.is_empty();
            observer.targets.retain(|target| target.id != id);
            observer.targets.push(Target {
                id,
                wrapper: target,
            });
            activate
        };
        if activate {
            registry.borrow_mut().active.push(this.0.clone());
        }
        Ok(())
    }

    fn unobserve(this: This<Class<'js, Self>>, ctx: Ctx<'js>, target: Object<'js>) -> Result<()> {
        let dom = registry(&ctx)?.borrow().dom.clone();
        let id = target_id(&ctx, &target, &dom)?;
        {
            let mut observer = this.0.borrow_mut();
            observer.native.unobserve(id);
            observer.targets.retain(|target| target.id != id);
        }
        remove_inactive(&ctx, &this.0)
    }

    fn disconnect(this: This<Class<'js, Self>>, ctx: Ctx<'js>) -> Result<()> {
        {
            let mut observer = this.0.borrow_mut();
            observer.native.disconnect();
            observer.targets.clear();
        }
        remove_inactive(&ctx, &this.0)
    }
}

fn remove_inactive<'js>(
    ctx: &Ctx<'js>,
    observer: &Class<'js, JsResizeObserver<'js>>,
) -> Result<()> {
    if observer.borrow().targets.is_empty() {
        registry(ctx)?
            .borrow_mut()
            .active
            .retain(|active| active != observer);
    }
    Ok(())
}

/// Uses the existing `app` binding to obtain the application's single DOM.
pub(crate) fn install(ctx: &Ctx<'_>) -> Result<()> {
    let app: Value = ctx.globals().get("app")?;
    let app = app
        .as_object()
        .and_then(Class::<NativeNode>::from_object)
        .ok_or_else(|| Exception::throw_type(ctx, "Install DomPlugin before ResizeObserver"))?;
    let dom = app.borrow().state.clone();
    if app.borrow().id != borrow(ctx, &dom)?.dom.root() {
        return Err(Exception::throw_type(
            ctx,
            "ResizeObserver requires the app root",
        ));
    }
    let prototype =
        Class::<JsResizeObserver>::prototype(ctx)?.expect("resize observer prototype exists");
    if prototype.contains_key(REGISTRY)? {
        return Err(Exception::throw_type(
            ctx,
            "ResizeObserver is already installed",
        ));
    }
    let object: Object = ctx.globals().get("Object")?;
    let registry = Class::instance(
        ctx.clone(),
        ObserverRegistry {
            dom,
            // Bare QuickJS contexts can still use the controlled delivery hook.
            signal: JsTaskQueue::from_context(ctx).ok().map(DeliverySignal::new),
            active: Vec::new(),
            freeze: object.get("freeze")?,
        },
    )?;
    Class::<JsResizeObserver>::define(&ctx.globals())?;
    prototype.prop(REGISTRY, Property::from(registry))?;
    Ok(())
}

struct PendingDelivery<'js> {
    observer: Class<'js, JsResizeObserver<'js>>,
    batch: ResizeBatch,
    targets: Vec<Object<'js>>,
    callback: Function<'js>,
}

/// Prepares all registrations before invoking any JS callback. The native commit
/// check rejects batches invalidated by an earlier callback or entry construction.
/// Runtime delivery tasks call this hook; bare-context tests can invoke it directly.
pub(crate) fn deliver(ctx: &Ctx<'_>) -> Result<()> {
    let (dom, observers, freeze) = {
        let registry = registry(ctx)?;
        let registry = registry.borrow();
        (
            registry.dom.clone(),
            registry.active.clone(),
            registry.freeze.clone(),
        )
    };
    let pending = {
        let state = borrow(ctx, &dom)?;
        let mut pending = Vec::new();
        for observer in observers {
            // Explicit native destruction removes registrations in the core.
            // Release their JS roots as well, even when no entry can be produced.
            observer
                .borrow_mut()
                .targets
                .retain(|target| state.dom.contains(target.id));
            let prepared = observer.borrow().native.prepare(&state.dom);
            let batch = match prepared {
                Err(ResizeObserverError::Closed) => {
                    observer.borrow_mut().targets.clear();
                    continue;
                }
                result => map_native(ctx, result)?,
            };
            let Some(batch) = batch else {
                continue;
            };
            let (targets, callback) = {
                let observer = observer.borrow();
                let targets = batch
                    .entries
                    .iter()
                    .map(|entry| {
                        observer
                            .targets
                            .iter()
                            .find(|target| target.id == entry.target)
                            .expect("native registration has a retained JS target")
                            .wrapper
                            .clone()
                    })
                    .collect();
                (targets, observer.callback.clone())
            };
            pending.push(PendingDelivery {
                observer,
                batch,
                targets,
                callback,
            });
        }
        pending
    };
    registry(ctx)?
        .borrow_mut()
        .active
        .retain(|observer| !observer.borrow().targets.is_empty());
    for pending in pending {
        let entries = make_entries(ctx, &freeze, &pending.batch.entries, pending.targets)?;
        let committed = {
            let state = borrow(ctx, &dom)?;
            let result = pending
                .observer
                .borrow()
                .native
                .commit(&state.dom, pending.batch);
            match result {
                Err(ResizeObserverError::Closed) => continue,
                result => map_native(ctx, result)?,
            }
        };
        if committed.is_empty() {
            continue;
        }
        if let Err(error) = pending
            .callback
            .call::<_, ()>((This(pending.observer.clone()), entries, pending.observer))
            .catch(ctx)
        {
            eprintln!("Burokku ResizeObserver callback failed: {error}");
        }
    }
    Ok(())
}

fn make_entries<'js>(
    ctx: &Ctx<'js>,
    freeze: &Function<'js>,
    records: &[ResizeObserverEntry],
    targets: Vec<Object<'js>>,
) -> Result<Array<'js>> {
    let entries = Array::new(ctx.clone())?;
    for (index, (record, target)) in records.iter().zip(targets).enumerate() {
        let size = Object::new(ctx.clone())?;
        size.prop("width", Property::from(record.size.width).enumerable())?;
        size.prop("height", Property::from(record.size.height).enumerable())?;
        freeze.call::<_, ()>((size.clone(),))?;
        let entry = Object::new(ctx.clone())?;
        entry.prop("target", Property::from(target).enumerable())?;
        entry.prop("size", Property::from(size).enumerable())?;
        freeze.call::<_, ()>((entry.clone(),))?;
        entries.set(index, entry)?;
    }
    freeze.call::<_, ()>((entries.clone(),))?;
    Ok(entries)
}

#[cfg(test)]
mod queued_tests {
    use super::*;
    use crate::{
        plugins::dom::DomPlugin,
        ui::{
            layout::{LayoutEngine, LogicalViewport},
            text::TextEngine,
        },
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        time::Duration,
    };
    use tokio::{
        sync::mpsc,
        task::{JoinHandle, LocalSet},
    };

    struct QueuedTest {
        runtime: runtime::Runtime,
        driver: JoinHandle<()>,
        bindings: SharedDomBindings,
        delivered: mpsc::UnboundedReceiver<f64>,
        signal: Rc<DeliverySignal>,
    }

    impl QueuedTest {
        async fn new(capacity: usize) -> Self {
            let (plugin, bindings) = DomPlugin::new_with_bindings();
            let (sender, delivered) = mpsc::unbounded_channel();
            let signal_slot = Rc::new(RefCell::new(None));
            let installed_signal = signal_slot.clone();
            let native = Rc::downgrade(&bindings);
            let (runtime, driver) = runtime::Runtime::builder()
                .macrotask_capacity(capacity)
                .plugin(plugin)
                .plugin(move |ctx: &Ctx<'_>| {
                    install(ctx)?;
                    *installed_signal.borrow_mut() = registry(ctx)?.borrow().signal.clone();
                    let sender = sender.clone();
                    ctx.globals().set(
                        "__recordResize",
                        Function::new(ctx.clone(), move |width: f64| {
                            let _ = sender.send(width);
                        })?,
                    )?;
                    let native = native.clone();
                    ctx.globals().set(
                        "__publishDuringCallback",
                        Function::new(ctx.clone(), move |width: f64| {
                            let bindings = native.upgrade().unwrap();
                            set_width(&bindings, width);
                            measure(&bindings);
                        })?,
                    )?;
                    Ok(())
                })
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            runtime
                .eval::<()>(
                    r#"
                globalThis.win = app.createElement('window');
                globalThis.panel = app.createElement('div');
                panel.style.setProperty('width', '100px');
                panel.style.setProperty('height', '50px');
                win.appendChild(panel);
                app.appendChild(win);
                globalThis.sizes = [];
                globalThis.observer = new ResizeObserver(entries => {
                    for (const entry of entries) {
                        sizes.push(entry.size.width);
                        __recordResize(entry.size.width);
                    }
                });
                observer.observe(panel);
            "#,
                )
                .await
                .unwrap();
            let signal = signal_slot.borrow_mut().take().unwrap();
            Self {
                runtime,
                driver,
                bindings,
                delivered,
                signal,
            }
        }

        async fn next(&mut self) -> f64 {
            tokio::time::timeout(Duration::from_secs(2), self.delivered.recv())
                .await
                .expect("resize delivery timed out")
                .expect("runtime closed")
        }

        async fn stop(self) {
            self.runtime.shutdown().await.unwrap();
            self.driver.await.unwrap();
        }
    }

    fn panel_id(bindings: &SharedDomBindings) -> NodeId {
        let state = bindings.borrow();
        let window = state.dom.first_child(state.dom.root()).unwrap().unwrap();
        state.dom.first_child(window).unwrap().unwrap()
    }

    fn set_width(bindings: &SharedDomBindings, width: f64) {
        let panel = panel_id(bindings);
        bindings
            .borrow_mut()
            .dom
            .set_style_property(panel, "width", &format!("{width}px"))
            .unwrap();
    }

    fn measure(bindings: &SharedDomBindings) {
        LayoutEngine::new(TextEngine::without_system_fonts())
            .compute(
                &bindings.borrow().dom,
                LogicalViewport::new(320.0, 240.0).unwrap(),
            )
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queued_delivery_coalesces_layouts_to_the_latest_size() {
        LocalSet::new()
            .run_until(async {
                let mut test = QueuedTest::new(8).await;
                for width in [100.0, 120.0, 150.0] {
                    set_width(&test.bindings, width);
                    measure(&test.bindings);
                }
                assert_eq!(test.runtime.macrotask_queue().depth(), 1);
                assert!(test.signal.pending.get());
                assert!(matches!(
                    test.delivered.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ));
                assert_eq!(test.next().await, 150.0);
                assert_eq!(
                    test.runtime.eval::<Vec<f64>>("sizes").await.unwrap(),
                    [150.0]
                );
                assert!(!test.signal.pending.get());
                test.stop().await;
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn full_queue_delivers_the_final_size_without_blocking_native_callbacks() {
        LocalSet::new()
            .run_until(async {
                let mut test = QueuedTest::new(1).await;
                let panel = panel_id(&test.bindings);
                let native = test.bindings.borrow().dom.resize_observer();
                let native_width = Rc::new(Cell::new(0.0));
                let recorded = native_width.clone();
                native
                    .set_callback(move |entries| recorded.set(entries[0].size.width))
                    .unwrap();
                native.observe(&test.bindings.borrow().dom, panel).unwrap();
                test.runtime
                    .macrotask_queue()
                    .try_enqueue(|_| Ok(()))
                    .unwrap();
                for width in [100.0, 130.0, 180.0] {
                    set_width(&test.bindings, width);
                    measure(&test.bindings);
                }
                assert_eq!(test.runtime.macrotask_queue().depth(), 1);
                let deliveries = {
                    let state = test.bindings.borrow();
                    state.dom.resize_observers.prepare_callbacks(&state.dom)
                };
                for delivery in deliveries {
                    let callback = delivery.commit(&test.bindings.borrow().dom);
                    if let Some(callback) = callback {
                        callback();
                    }
                }
                assert_eq!(native_width.get(), 180.0);
                assert!(matches!(
                    test.delivered.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ));
                // No further host/layout work occurs while capacity becomes available.
                assert_eq!(test.next().await, 180.0);
                assert_eq!(
                    test.runtime.eval::<Vec<f64>>("sizes").await.unwrap(),
                    [180.0]
                );
                test.stop().await;
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queued_disconnect_cancels_delivery_and_reobserve_remains_reusable() {
        LocalSet::new()
            .run_until(async {
                let mut test = QueuedTest::new(8).await;
                test.runtime
                    .macrotask_queue()
                    .try_enqueue(|ctx| ctx.eval::<(), _>("observer.disconnect()"))
                    .unwrap();
                measure(&test.bindings);
                assert_eq!(test.runtime.eval::<i32>("sizes.length").await.unwrap(), 0);
                test.runtime
                    .eval::<()>("observer.observe(panel)")
                    .await
                    .unwrap();
                measure(&test.bindings);
                assert_eq!(test.next().await, 100.0);
                test.stop().await;
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn layout_published_during_a_js_callback_gets_another_task() {
        LocalSet::new()
            .run_until(async {
                let mut test = QueuedTest::new(8).await;
                test.runtime
                    .eval::<()>(
                        r#"
                observer.disconnect();
                observer = new ResizeObserver(entries => {
                    const width = entries[0].size.width;
                    sizes.push(width);
                    __recordResize(width);
                    if (width === 100) __publishDuringCallback(150);
                });
                observer.observe(panel);
            "#,
                    )
                    .await
                    .unwrap();
                measure(&test.bindings);
                assert_eq!(test.next().await, 100.0);
                assert_eq!(test.next().await, 150.0);
                assert_eq!(
                    test.runtime.eval::<Vec<f64>>("sizes").await.unwrap(),
                    [100.0, 150.0]
                );
                test.stop().await;
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn conversion_failure_retries_once_and_preserves_records_after_persistent_failure() {
        LocalSet::new().run_until(async {
            let mut test = QueuedTest::new(8).await;
            test.runtime.macrotask_queue().try_enqueue(|ctx| {
                registry(ctx)?.borrow_mut().freeze = ctx.eval(r#"value => {
                    if (!globalThis.failedOnce) { globalThis.failedOnce = true; throw new Error('temporary'); }
                    return Object.freeze(value);
                }"#)?;
                Ok(())
            }).unwrap();
            measure(&test.bindings);
            assert_eq!(test.next().await, 100.0);
            test.runtime.macrotask_queue().try_enqueue(|ctx| {
                registry(ctx)?.borrow_mut().freeze = ctx.eval("() => { __recordResize(-1); throw new Error('persistent'); }")?;
                Ok(())
            }).unwrap();
            set_width(&test.bindings, 150.0);
            measure(&test.bindings);
            assert_eq!(test.next().await, -1.0);
            assert_eq!(test.next().await, -1.0);
            assert_eq!(test.runtime.eval::<Vec<f64>>("sizes").await.unwrap(), [100.0]);
            assert!(!test.signal.pending.get());
            assert!(matches!(test.delivered.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
            test.runtime.macrotask_queue().try_enqueue(|ctx| {
                registry(ctx)?.borrow_mut().freeze = ctx.eval("Object.freeze")?;
                Ok(())
            }).unwrap();
            test.signal.request();
            assert_eq!(test.next().await, 150.0);
            test.stop().await;
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queue_closure_cancels_a_capacity_waiter_and_releases_js_subscriptions() {
        LocalSet::new()
            .run_until(async {
                let test = QueuedTest::new(1).await;
                let signal = test.signal.clone();
                let bindings = test.bindings.clone();
                test.runtime
                    .macrotask_queue()
                    .try_enqueue(|_| Ok(()))
                    .unwrap();
                measure(&bindings);
                assert!(signal.pending.get());
                test.stop().await;
                assert!(signal.closed.get());
                assert!(!signal.pending.get());
                assert!(bindings
                    .borrow()
                    .dom
                    .resize_observers
                    .retained_targets()
                    .is_empty());
                signal.request();
                assert!(!signal.pending.get());
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_shutdown_discards_already_queued_js_work() {
        LocalSet::new()
            .run_until(async {
                let test = QueuedTest::new(8).await;
                measure(&test.bindings);
                test.bindings.borrow().dom.resize_observers.shutdown();
                assert_eq!(test.runtime.eval::<i32>("sizes.length").await.unwrap(), 0);
                assert!(!test.signal.pending.get());
                test.stop().await;
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime as JsRuntime};
    use runtime::Plugin;

    use super::*;
    use crate::{
        plugins::dom::DomPlugin,
        ui::{
            elements::ElementTag,
            layout::{LayoutEngine, LogicalViewport},
            text::TextEngine,
        },
    };

    fn setup() -> (JsRuntime, Context, DomPlugin, SharedDomBindings) {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        let (plugin, bindings) = DomPlugin::new_with_bindings();
        context.with(|ctx| {
            plugin.install(&ctx).unwrap();
            install(&ctx).catch(&ctx).unwrap();
        });
        (runtime, context, plugin, bindings)
    }

    fn eval(context: &Context, source: &str) {
        context.with(|ctx| ctx.eval::<(), _>(source).catch(&ctx).unwrap());
    }

    fn check(context: &Context, source: &str) {
        context.with(|ctx| assert!(ctx.eval::<bool, _>(source).catch(&ctx).unwrap(), "{source}"));
    }

    fn collect(runtime: &JsRuntime, context: &Context) {
        for _ in 0..3 {
            runtime.run_gc();
            context.with(|ctx| while ctx.execute_pending_job() {});
        }
    }

    fn measure(bindings: &SharedDomBindings) {
        LayoutEngine::new(TextEngine::without_system_fonts())
            .compute(
                &bindings.borrow().dom,
                LogicalViewport::new(320.0, 240.0).unwrap(),
            )
            .unwrap();
    }

    fn deliver_now(context: &Context) {
        context.with(|ctx| deliver(&ctx).catch(&ctx).unwrap());
    }

    fn mount(context: &Context) {
        eval(
            context,
            r#"
            globalThis.win = app.createElement('window');
            globalThis.panel = app.createElement('div');
            panel.style.setProperty('width', '100px');
            panel.style.setProperty('height', '50px');
            panel.style.setProperty('padding', '10px');
            win.appendChild(panel);
            app.appendChild(win);
        "#,
        );
    }

    #[test]
    fn install_requires_dom_and_rejects_duplicate_installation() {
        let runtime = JsRuntime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            assert!(install(&ctx).catch(&ctx).is_err());
            DomPlugin::new().install(&ctx).unwrap();
            install(&ctx).unwrap();
            assert!(install(&ctx).catch(&ctx).is_err());
        });
    }

    #[test]
    fn validates_callbacks_targets_and_unsupported_options() {
        let (_runtime, context, _plugin, bindings) = setup();
        check(
            &context,
            r#"(() => {
            const observer = new ResizeObserver(() => {});
            const panel = app.createElement('div');
            const badCalls = [
                () => new ResizeObserver(),
                () => new ResizeObserver(12),
                () => observer.observe(app),
                () => observer.observe(app.createTextNode('raw text')),
                () => observer.observe({}),
                () => observer.observe(null),
                () => observer.unobserve(app),
                () => observer.observe(panel, {box: 'border-box'}),
                () => observer.observe(panel, {}),
                () => observer.observe(panel, null),
            ];
            for (const call of badCalls) {
                try { call(); return false; }
                catch (error) { if (!(error instanceof TypeError)) return false; }
            }
            observer.observe(panel, undefined);
            observer.observe(app.createElement('text'));
            observer.disconnect();
            return true;
        })()"#,
        );
        assert!(bindings
            .borrow()
            .dom
            .resize_observers
            .retained_targets()
            .is_empty());
    }

    #[test]
    fn entries_use_canonical_wrappers_and_native_sizes_with_immutable_snapshots() {
        let (_runtime, context, _plugin, bindings) = setup();
        mount(&context);
        eval(
            &context,
            r#"
            globalThis.calls = 0;
            globalThis.observer = new ResizeObserver(function(entries, current) {
                globalThis.receiverMatches = this === observer && current === observer;
                globalThis.lastEntries = entries;
                calls++;
            });
            observer.observe(win);
            observer.observe(panel);
        "#,
        );
        measure(&bindings);
        check(&context, "calls === 0");
        deliver_now(&context);
        check(
            &context,
            r#"
            calls === 1 && receiverMatches && lastEntries.length === 2 &&
            lastEntries[0].target === win && lastEntries[0].size.width === 320 &&
            lastEntries[1].target === panel && lastEntries[1].size.width === 100 &&
            lastEntries[1].size.height === 50 &&
            Object.keys(lastEntries[1]).join(',') === 'target,size' &&
            Object.isFrozen(lastEntries) && Object.isFrozen(lastEntries[1]) &&
            Object.isFrozen(lastEntries[1].size)
        "#,
        );
        eval(&context, "panel.style.setProperty('margin', '5px')");
        measure(&bindings);
        deliver_now(&context);
        check(&context, "calls === 1");
        eval(&context, "panel.style.setProperty('width', '150px')");
        measure(&bindings);
        deliver_now(&context);
        check(&context, "calls === 2 && lastEntries.length === 1 && lastEntries[0].target === panel && lastEntries[0].size.width === 150");
    }

    #[test]
    fn earlier_js_callback_can_cancel_and_reobserve_a_later_pending_batch() {
        let (_runtime, context, _plugin, bindings) = setup();
        mount(&context);
        eval(
            &context,
            r#"
            globalThis.log = [];
            globalThis.first = new ResizeObserver(() => {
                log.push('first');
                second.disconnect();
                second.observe(panel);
            });
            globalThis.second = new ResizeObserver(() => log.push('second'));
            first.observe(panel);
            second.observe(panel);
        "#,
        );
        measure(&bindings);
        deliver_now(&context);
        check(&context, "log.join(',') === 'first'");
        measure(&bindings);
        deliver_now(&context);
        check(&context, "log.join(',') === 'first,second'");
        eval(
            &context,
            "first.disconnect(); second.unobserve(panel); second.unobserve(panel);",
        );
        assert!(bindings
            .borrow()
            .dom
            .resize_observers
            .retained_targets()
            .is_empty());
    }

    #[test]
    fn failed_entry_creation_does_not_consume_native_records() {
        let (_runtime, context, _plugin, bindings) = setup();
        mount(&context);
        eval(&context, "globalThis.calls = 0; globalThis.observer = new ResizeObserver(() => calls++); observer.observe(panel)");
        measure(&bindings);
        context.with(|ctx| {
            let registry = registry(&ctx).unwrap();
            let freeze = registry.borrow().freeze.clone();
            registry.borrow_mut().freeze = ctx
                .eval("() => { throw new Error('injected conversion failure'); }")
                .unwrap();
            assert!(deliver(&ctx).catch(&ctx).is_err());
            registry.borrow_mut().freeze = freeze;
        });
        check(&context, "calls === 0");
        deliver_now(&context);
        check(&context, "calls === 1");
        deliver_now(&context);
        check(&context, "calls === 1");
    }

    #[test]
    fn callback_exceptions_do_not_suppress_other_observers() {
        let (_runtime, context, _plugin, bindings) = setup();
        mount(&context);
        eval(
            &context,
            r#"
            globalThis.calls = 0;
            globalThis.bad = new ResizeObserver(() => { throw new Error('callback failed'); });
            globalThis.good = new ResizeObserver(() => calls++);
            bad.observe(panel);
            good.observe(panel);
        "#,
        );
        measure(&bindings);
        deliver_now(&context);
        check(&context, "calls === 1");
    }

    #[test]
    fn gc_retains_active_observers_and_targets_then_collects_disconnected_cycles() {
        let (runtime, context, plugin, bindings) = setup();
        eval(
            &context,
            r#"(() => {
            const target = app.createElement('div');
            const observer = new ResizeObserver(() => { void target; void observer; });
            observer.observe(target);
            globalThis.observerWeak = new WeakRef(observer);
            globalThis.targetWeak = new WeakRef(target);
        })()"#,
        );
        collect(&runtime, &context);
        assert_eq!(
            bindings
                .borrow()
                .dom
                .resize_observers
                .retained_targets()
                .len(),
            1
        );
        assert!(plugin.reclaim_for_test().nodes.is_empty());
        check(
            &context,
            "observerWeak.deref() !== undefined && targetWeak.deref() !== undefined",
        );
        eval(&context, "observerWeak.deref().disconnect()");
        collect(&runtime, &context);
        assert!(bindings
            .borrow()
            .dom
            .resize_observers
            .retained_targets()
            .is_empty());
        assert_eq!(plugin.reclaim_for_test().nodes.len(), 1);
        check(
            &context,
            "observerWeak.deref() === undefined && targetWeak.deref() === undefined",
        );
    }

    #[test]
    fn unobserve_releases_only_that_target_and_passive_observers_are_collectible() {
        let (runtime, context, plugin, bindings) = setup();
        eval(
            &context,
            r#"(() => {
            const first = app.createElement('div');
            const second = app.createElement('div');
            globalThis.observer = new ResizeObserver(() => {});
            observer.observe(first);
            observer.observe(second);
            globalThis.firstWeak = new WeakRef(first);
            globalThis.secondWeak = new WeakRef(second);
            const passive = new ResizeObserver(() => passive);
            globalThis.passiveWeak = new WeakRef(passive);
        })()"#,
        );
        eval(&context, "observer.unobserve(firstWeak.deref())");
        collect(&runtime, &context);
        assert_eq!(plugin.reclaim_for_test().nodes.len(), 1);
        assert_eq!(
            bindings
                .borrow()
                .dom
                .resize_observers
                .retained_targets()
                .len(),
            1
        );
        check(&context, "firstWeak.deref() === undefined && secondWeak.deref() !== undefined && passiveWeak.deref() === undefined");
        eval(&context, "observer.disconnect()");
    }

    #[test]
    fn native_destruction_releases_adapter_roots_on_the_next_delivery_check() {
        let (runtime, context, _plugin, bindings) = setup();
        eval(
            &context,
            r#"(() => {
            const target = app.createElement('div');
            const observer = new ResizeObserver(() => {});
            observer.observe(target);
            globalThis.observerWeak = new WeakRef(observer);
        })()"#,
        );
        let target = bindings.borrow().dom.resize_observers.retained_targets()[0];
        bindings.borrow_mut().dom.remove_subtree(target).unwrap();
        measure(&bindings);
        deliver_now(&context);
        collect(&runtime, &context);
        check(&context, "observerWeak.deref() === undefined");
    }

    #[test]
    fn runtime_teardown_releases_js_subscriptions_without_cancelling_native_ones() {
        let (runtime, context, _plugin, bindings) = setup();
        let native = {
            let mut state = bindings.borrow_mut();
            let target = state.dom.create_element_tag(ElementTag::Div);
            let observer = state.dom.resize_observer();
            observer.observe(&state.dom, target).unwrap();
            observer
        };
        eval(
            &context,
            "new ResizeObserver(() => {}).observe(app.createElement('div'))",
        );
        assert_eq!(
            bindings
                .borrow()
                .dom
                .resize_observers
                .retained_targets()
                .len(),
            2
        );
        // rquickjs retains class prototypes in runtime-owned storage. Drop the
        // runtime as well as its context before checking teardown of those roots.
        drop(context);
        drop(runtime);
        assert_eq!(
            bindings
                .borrow()
                .dom
                .resize_observers
                .retained_targets()
                .len(),
            1
        );
        drop(native);
        assert!(bindings
            .borrow()
            .dom
            .resize_observers
            .retained_targets()
            .is_empty());
    }
}
