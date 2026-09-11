use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use tokio::sync::oneshot;
use winit::{ElementState, MouseButton, PhysicalPosition, PhysicalSize};

use crate::{
    app::RuntimeLifecycle,
    ui::{
        elements::{Dom, Element, ElementTag, NodeId},
        gpu::GraphicsError,
        layout::{LayoutEngine, LogicalViewport},
        scene::ScenePlan,
        text::TextEngine,
    },
};

use super::{
    events::{keyboard_key, pointer_input_for_input, wheel_input_for_input},
    gpu_lifecycle::{
        accept_current_graphics_result, cancel_graphics_task, classify_replacement_renderer,
        graphics_initialization_stop_is_fatal, pending_window_status, AbortOnDrop,
        PendingWindowStatus, ReplacementRenderer,
    },
    render::{
        classify_candidate_failure, classify_fatal_failure, classify_resize_failure,
        failure_policy, finish_successful_presentation, logical_viewport, presentation_state,
        presented_frame_is_usable, FailureKind, FailurePolicy, FrameFailure, FrameStage,
        PresentationState, PresentedFrame, PresentedSurface, RedrawFailure,
    },
    ApplicationHost, ChangedMouseButton, HostError, NativeMouseInput, NativeMouseInputKind,
    PressedMouseButtons, WheelDeltaMode,
};

fn test_mouse_input(
    target: Option<NodeId>,
    position: PhysicalPosition<f64>,
    buttons: u16,
    presented: (u64, f64),
    event_type: Option<&'static str>,
) -> NativeMouseInput {
    NativeMouseInput {
        kind: match event_type {
            None => NativeMouseInputKind::Hover,
            Some("pointerdown") => NativeMouseInputKind::Button {
                button: ChangedMouseButton::PRIMARY,
                pressed: true,
            },
            Some("pointerup") => NativeMouseInputKind::Button {
                button: ChangedMouseButton::PRIMARY,
                pressed: false,
            },
            Some("pointermove") => NativeMouseInputKind::Move,
            Some(event_type) => panic!("unsupported test mouse input: {event_type}"),
        },
        hit_target: target,
        presented_revision: presented.0,
        client_x: position.x / presented.1,
        client_y: position.y / presented.1,
        buttons: PressedMouseButtons::from_bits(buttons),
    }
}

fn queue_test_mouse(
    host: &mut ApplicationHost,
    target: Option<NodeId>,
    position: PhysicalPosition<f64>,
    buttons: u16,
    presented: (u64, f64),
    event_type: Option<&'static str>,
) -> Result<(), HostError> {
    host.queue_pointer_input(
        test_mouse_input(target, position, buttons, presented, event_type),
        presented.1,
    )
}

#[test]
fn keyboard_text_uses_dom_key_names() {
    assert_eq!(keyboard_key(0, Some("a".into()), None), "a");
    assert_eq!(keyboard_key(0, Some("\r".into()), None), "Enter");
    assert_eq!(keyboard_key(0, Some("\u{f700}".into()), None), "ArrowUp");
    assert_eq!(keyboard_key(0x38, None, None), "Shift");
    assert_eq!(keyboard_key(0, None, None), "Unidentified");
}

#[test]
fn control_c_is_not_mistaken_for_enter() {
    // AppKit reports U+0003 in characters() and "c" in charactersIgnoringModifiers().
    assert_eq!(
        keyboard_key(0x08, Some("\u{3}".into()), Some("c".into())),
        "c"
    );
}

#[derive(Debug)]
struct DropProbe {
    name: &'static str,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.name);
    }
}

fn drop_probe(name: &'static str, events: &Arc<Mutex<Vec<&'static str>>>) -> DropProbe {
    DropProbe {
        name,
        events: Arc::clone(events),
    }
}
fn scene_plan(dom: &Dom) -> ScenePlan {
    let mut layout = LayoutEngine::new(TextEngine::without_system_fonts());
    let computed = layout
        .compute(dom, LogicalViewport::new(320.0, 240.0).unwrap())
        .unwrap();
    ScenePlan::from_layout(dom, computed, PhysicalSize::new(320, 240), 1.0).unwrap()
}

#[test]
fn pointer_motion_queues_a_native_move_fact() {
    let input = pointer_input_for_input(
        &scene_plan(&Dom::new()),
        PhysicalPosition::new(0.0, 0.0),
        0,
        None,
    );

    assert_eq!(input.kind, NativeMouseInputKind::Move);
}

fn oversized_target() -> GraphicsError {
    GraphicsError::TargetTooLarge {
        size: PhysicalSize::new(70_000, 10),
        max_texture_dimension_2d: 16_384,
        max_vello_dimension: 65_535,
    }
}

fn test_surface(
    window_id: u8,
    physical_size: PhysicalSize<u32>,
    generation: u64,
) -> PresentedSurface<u8> {
    PresentedSurface {
        window_id,
        physical_size,
        generation,
    }
}

#[test]
fn windowless_host_does_not_initialize_graphics() {
    let (_plugin, dom) = crate::plugins::dom::DomPlugin::new_with_bindings();
    let lifecycle = RuntimeLifecycle::for_test();
    let host = ApplicationHost::new(dom, TextEngine::without_system_fonts(), lifecycle);

    assert!(host.graphics.is_none());
    assert!(host.pending_graphics.is_none());
    assert!(host.renderer.is_none());
}

fn measurement_host() -> (ApplicationHost, NodeId, NodeId) {
    let (_plugin, bindings) = crate::plugins::dom::DomPlugin::new_with_bindings();
    let (window, panel) = {
        let mut state = bindings.borrow_mut();
        let dom = &mut state.dom;
        let window = dom.create_element_tag(ElementTag::Window);
        let panel = dom.create_element_tag(ElementTag::Div);
        dom.set_style_property(panel, "width", "100px").unwrap();
        dom.set_style_property(panel, "height", "50px").unwrap();
        dom.append_child(window, panel).unwrap();
        dom.append_child(dom.root(), window).unwrap();
        (window, panel)
    };
    (
        ApplicationHost::new(
            bindings,
            TextEngine::without_system_fonts(),
            RuntimeLifecycle::for_test(),
        ),
        window,
        panel,
    )
}

#[test]
#[should_panic(expected = "DOM must not be mutably borrowed when preparing resize callbacks")]
fn resize_delivery_panics_if_its_dom_borrow_invariant_is_violated() {
    let (mut host, _, _) = measurement_host();
    let bindings = host.dom_bindings.clone();
    let _borrow = bindings.borrow_mut();
    host.deliver_resize_callbacks();
}

#[test]
fn native_resize_callbacks_defer_mutations_until_the_next_measurement() {
    let (mut host, _, panel) = measurement_host();
    let bindings = Rc::downgrade(&host.dom_bindings);
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let observer = host.dom_bindings.borrow().dom.resize_observer();
    let recorded = sizes.clone();
    observer
        .set_callback(move |entries| {
            recorded.borrow_mut().push(entries[0].size.width);
            // This would panic if the host retained its DOM borrow across callbacks.
            let bindings = bindings.upgrade().unwrap();
            let mut state = bindings.borrow_mut();
            if entries[0].size.width == 100.0 {
                state
                    .dom
                    .set_style_property(panel, "width", "150px")
                    .unwrap();
            }
        })
        .unwrap();
    observer
        .observe(&host.dom_bindings.borrow().dom, panel)
        .unwrap();
    let size = PhysicalSize::new(320, 240);
    host.measure_layout(size, 1.0).unwrap();
    assert!(sizes.borrow().is_empty());
    host.deliver_resize_callbacks();
    assert_eq!(*sizes.borrow(), [100.0]);
    assert!(host
        .dom_bindings
        .borrow()
        .dom
        .resize_observers
        .measurement_requested());
    host.deliver_resize_callbacks();
    assert_eq!(*sizes.borrow(), [100.0]);
    host.measure_layout(size, 1.0).unwrap();
    host.deliver_resize_callbacks();
    assert_eq!(*sizes.borrow(), [100.0, 150.0]);
    assert!(!host
        .dom_bindings
        .borrow()
        .dom
        .resize_observers
        .measurement_requested());
}

#[test]
fn native_callback_added_after_idle_layout_uses_the_cache_and_only_fires_once() {
    let (mut host, _, panel) = measurement_host();
    let size = PhysicalSize::new(320, 240);
    let cached = host.measure_layout(size, 1.0).unwrap();
    let observer = host.dom_bindings.borrow().dom.resize_observer();
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let recorded = sizes.clone();
    observer
        .set_callback(move |entries| recorded.borrow_mut().push(entries[0].size))
        .unwrap();
    observer
        .observe(&host.dom_bindings.borrow().dom, panel)
        .unwrap();
    assert!(host
        .dom_bindings
        .borrow()
        .dom
        .resize_observers
        .measurement_requested());
    assert!(Rc::ptr_eq(
        &cached,
        &host.measure_layout(size, 1.0).unwrap()
    ));
    host.deliver_resize_callbacks();
    host.measure_layout(size, 1.0).unwrap();
    host.deliver_resize_callbacks();
    assert_eq!(sizes.borrow().len(), 1);
}

#[test]
fn native_callback_mutation_invalidates_later_batches_until_remeasurement() {
    let (mut host, _, panel) = measurement_host();
    let first = host.dom_bindings.borrow().dom.resize_observer();
    let second = host.dom_bindings.borrow().dom.resize_observer();
    let bindings = Rc::downgrade(&host.dom_bindings);
    let sizes = Rc::new(RefCell::new(Vec::new()));
    first
        .set_callback(move |_| {
            bindings
                .upgrade()
                .unwrap()
                .borrow_mut()
                .dom
                .set_style_property(panel, "width", "150px")
                .unwrap();
        })
        .unwrap();
    let recorded = sizes.clone();
    second
        .set_callback(move |entries| recorded.borrow_mut().push(entries[0].size.width))
        .unwrap();
    for observer in [&first, &second] {
        observer
            .observe(&host.dom_bindings.borrow().dom, panel)
            .unwrap();
    }
    host.measure_layout(PhysicalSize::new(320, 240), 1.0)
        .unwrap();
    host.deliver_resize_callbacks();
    assert!(sizes.borrow().is_empty());
    host.measure_layout(PhysicalSize::new(320, 240), 1.0)
        .unwrap();
    host.deliver_resize_callbacks();
    assert_eq!(*sizes.borrow(), [150.0]);
}

#[test]
fn partial_unobserve_during_an_earlier_callback_retries_remaining_entries() {
    let (mut host, window, panel) = measurement_host();
    let first = host.dom_bindings.borrow().dom.resize_observer();
    let second = Rc::new(host.dom_bindings.borrow().dom.resize_observer());
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let later = second.clone();
    first.set_callback(move |_| later.unobserve(panel)).unwrap();
    let recorded = delivered.clone();
    second
        .set_callback(move |entries| {
            recorded
                .borrow_mut()
                .push(entries.iter().map(|entry| entry.target).collect::<Vec<_>>());
        })
        .unwrap();
    first
        .observe(&host.dom_bindings.borrow().dom, panel)
        .unwrap();
    for target in [panel, window] {
        second
            .observe(&host.dom_bindings.borrow().dom, target)
            .unwrap();
    }

    let size = PhysicalSize::new(320, 240);
    host.measure_layout(size, 1.0).unwrap();
    host.deliver_resize_callbacks();

    assert!(delivered.borrow().is_empty());
    assert!(host
        .dom_bindings
        .borrow()
        .dom
        .resize_observers
        .measurement_requested());

    host.measure_layout(size, 1.0).unwrap();
    host.deliver_resize_callbacks();
    assert_eq!(*delivered.borrow(), [vec![window]]);
}

#[test]
fn replacing_a_native_callback_invalidates_its_prepared_delivery() {
    let (mut host, _, panel) = measurement_host();
    let first = host.dom_bindings.borrow().dom.resize_observer();
    let second = Rc::new(host.dom_bindings.borrow().dom.resize_observer());
    let log = Rc::new(RefCell::new(Vec::new()));
    let recorded = log.clone();
    second
        .set_callback(move |_| recorded.borrow_mut().push("old"))
        .unwrap();
    let later = second.clone();
    let recorded = log.clone();
    first
        .set_callback(move |_| {
            let recorded = recorded.clone();
            later
                .set_callback(move |_| recorded.borrow_mut().push("new"))
                .unwrap();
        })
        .unwrap();
    for observer in [&first, second.as_ref()] {
        observer
            .observe(&host.dom_bindings.borrow().dom, panel)
            .unwrap();
    }
    let size = PhysicalSize::new(320, 240);
    host.measure_layout(size, 1.0).unwrap();
    host.deliver_resize_callbacks();
    assert!(log.borrow().is_empty());
    host.measure_layout(size, 1.0).unwrap();
    host.deliver_resize_callbacks();
    assert_eq!(*log.borrow(), ["new"]);
}

#[test]
fn dropping_a_native_observer_cancels_its_already_prepared_callback() {
    let (mut host, _, panel) = measurement_host();
    let first = host.dom_bindings.borrow().dom.resize_observer();
    let second = host.dom_bindings.borrow().dom.resize_observer();
    second
        .set_callback(|_| panic!("cancelled observer must not fire"))
        .unwrap();
    for observer in [&first, &second] {
        observer
            .observe(&host.dom_bindings.borrow().dom, panel)
            .unwrap();
    }
    let second = Rc::new(RefCell::new(Some(second)));
    let cancelled = second.clone();
    first
        .set_callback(move |_| {
            cancelled.borrow_mut().take();
        })
        .unwrap();
    host.measure_layout(PhysicalSize::new(320, 240), 1.0)
        .unwrap();
    host.deliver_resize_callbacks();
    assert!(second.borrow().is_none());
}

#[test]
fn dropping_host_releases_native_callbacks_and_closes_surviving_handles() {
    let (mut host, _, panel) = measurement_host();
    let bindings = host.dom_bindings.clone();
    let observer = bindings.borrow().dom.resize_observer();
    let retained = Rc::new(());
    let weak = Rc::downgrade(&retained);
    observer
        .set_callback(move |_| {
            let _ = &retained;
        })
        .unwrap();
    observer.observe(&bindings.borrow().dom, panel).unwrap();
    host.measure_layout(PhysicalSize::new(320, 240), 1.0)
        .unwrap();
    let state = bindings.borrow_mut();
    drop(host);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        observer.take_records(&state.dom),
        Err(crate::ui::resize_observer::ResizeObserverError::Closed)
    );
}

#[test]
fn host_measurement_reuses_layout_without_gpu_or_updating_presented_geometry() {
    let (mut host, _, panel) = measurement_host();
    let observer = {
        let state = host.dom_bindings.borrow();
        let observer = state.dom.resize_observer();
        observer.observe(&state.dom, panel).unwrap();
        observer
    };
    let initial = host
        .measure_layout(PhysicalSize::new(640, 480), 2.0)
        .unwrap();
    let cached = host
        .measure_layout(PhysicalSize::new(640, 480), 2.0)
        .unwrap();
    assert!(std::rc::Rc::ptr_eq(&initial, &cached));
    {
        let mut state = host.dom_bindings.borrow_mut();
        assert!(state.layout_rect(panel).unwrap().is_none());
        // Simulate the previously presented layout. Measurement must not replace it.
        state.publish_presented_layout(initial);
        observer.take_records(&state.dom).unwrap();
        state
            .dom
            .set_style_property(panel, "width", "150px")
            .unwrap();
    }
    let measured = host
        .measure_layout(PhysicalSize::new(800, 480), 2.0)
        .unwrap();
    assert_eq!(measured.viewport().width(), 400.0);
    let state = host.dom_bindings.borrow();
    assert_eq!(
        observer.take_records(&state.dom).unwrap()[0].size.width,
        150.0
    );
    assert_eq!(state.layout_rect(panel).unwrap().unwrap().width, 100.0);
    assert!(host.graphics.is_none());
    assert!(host.renderer.is_none());
    assert!(host.presented.is_none());
}

#[test]
fn host_measures_zero_viewport_without_collapsing_fixed_children() {
    let (mut host, window, panel) = measurement_host();
    let observer = {
        let state = host.dom_bindings.borrow();
        let observer = state.dom.resize_observer();
        observer.observe(&state.dom, window).unwrap();
        observer.observe(&state.dom, panel).unwrap();
        observer
    };
    host.measure_layout(PhysicalSize::new(320, 240), 1.0)
        .unwrap();
    observer
        .take_records(&host.dom_bindings.borrow().dom)
        .unwrap();
    let measured = host.measure_layout(PhysicalSize::new(0, 0), 2.0).unwrap();
    let entries = observer
        .take_records(&host.dom_bindings.borrow().dom)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].target, window);
    assert_eq!(entries[0].size, taffy::geometry::Size::ZERO);
    assert_eq!(
        measured.box_for(panel).unwrap().layout().size,
        taffy::geometry::Size {
            width: 100.0,
            height: 50.0
        }
    );
    assert!(host.renderer.is_none());
}

#[test]
fn host_measurements_survive_a_target_too_large_to_present() {
    let (mut host, window, _) = measurement_host();
    let observer = {
        let state = host.dom_bindings.borrow();
        let observer = state.dom.resize_observer();
        observer.observe(&state.dom, window).unwrap();
        observer
    };
    let size = PhysicalSize::new(70_000, 240);
    let measured = host.measure_layout(size, 1.0).unwrap();
    let state = host.dom_bindings.borrow();
    assert!(matches!(
        ScenePlan::from_layout(&state.dom, &measured, size, 1.0),
        Err(crate::ui::scene::SceneError::TargetTooLarge { .. })
    ));
    assert_eq!(
        observer.take_records(&state.dom).unwrap()[0].size.width,
        70_000.0
    );
    assert!(state.layout_rect(window).unwrap().is_none());
    assert!(host.presented.is_none());
}

#[test]
fn failed_host_measurement_preserves_previous_layout_and_observer_state() {
    let (mut host, _, panel) = measurement_host();
    let observer = {
        let state = host.dom_bindings.borrow();
        let observer = state.dom.resize_observer();
        observer.observe(&state.dom, panel).unwrap();
        observer
    };
    let size = PhysicalSize::new(320, 240);
    let previous = host.measure_layout(size, 1.0).unwrap();
    let deep_root = {
        let mut state = host.dom_bindings.borrow_mut();
        observer.take_records(&state.dom).unwrap();
        state.publish_presented_layout(previous.clone());
        let dom = &mut state.dom;
        dom.set_style_property(panel, "width", "150px").unwrap();
        let deep_root = dom.create_element_tag(ElementTag::Div);
        dom.append_child(panel, deep_root).unwrap();
        let mut parent = deep_root;
        for _ in 0..260 {
            let child = dom.create_element_tag(ElementTag::Div);
            dom.append_child(parent, child).unwrap();
            parent = child;
        }
        deep_root
    };
    assert!(matches!(
        host.measure_layout(size, 1.0),
        Err(RedrawFailure::Fatal(HostError::Layout(
            crate::ui::layout::LayoutError::TreeTooDeep { .. }
        )))
    ));
    assert!(std::rc::Rc::ptr_eq(
        &previous,
        &host.layout.current_shared().unwrap()
    ));
    assert!(!host
        .dom_bindings
        .borrow()
        .dom
        .resize_observers
        .measurement_requested());
    {
        let mut state = host.dom_bindings.borrow_mut();
        assert!(observer.take_records(&state.dom).unwrap().is_empty());
        assert_eq!(state.layout_rect(panel).unwrap().unwrap().width, 100.0);
        state.dom.detach(deep_root).unwrap();
    }
    host.measure_layout(size, 1.0).unwrap();
    let state = host.dom_bindings.borrow();
    assert_eq!(
        observer.take_records(&state.dom).unwrap()[0].size.width,
        150.0
    );
    assert_eq!(state.layout_rect(panel).unwrap().unwrap().width, 100.0);
}

#[test]
fn pending_window_status_detects_removal_replacement_and_same_window_updates() {
    let mut dom = Dom::new();
    let window_a = dom.create_element(Element::from_tag(ElementTag::Window));
    let window_b = dom.create_element(Element::from_tag(ElementTag::Window));

    assert_eq!(
        pending_window_status(window_a, Some(window_a)),
        PendingWindowStatus::Current
    );
    assert_eq!(
        pending_window_status(window_a, None),
        PendingWindowStatus::Removed
    );
    assert_eq!(
        pending_window_status(window_a, Some(window_b)),
        PendingWindowStatus::Replaced
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hover_transitions_handle_ancestors_reparenting_and_detachment() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (plugin, state) = crate::plugins::dom::DomPlugin::new_with_bindings();
            let (runtime, driver) = runtime::Runtime::builder()
                .plugin(plugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let mut host = ApplicationHost::new(
                state.clone(),
                TextEngine::without_system_fonts(),
                RuntimeLifecycle::for_test(),
            );
            runtime
                .eval::<()>(
                    r#"
            globalThis.w = app.createElement('window');
            globalThis.p = app.createElement('div');
            globalThis.q = app.createElement('div');
            globalThis.a = app.createElement('div');
            globalThis.b = app.createElement('div');
            app.appendChild(w); w.appendChild(p); w.appendChild(q);
            p.appendChild(a); p.appendChild(b);
            globalThis.hoverLog = []; globalThis.hoverChecks = [];
            for (const [name, node] of Object.entries({app, w, p, q, a, b})) {
                node.testName = name;
                for (const type of ['pointerenter', 'pointerleave']) {
                    node.addEventListener(type, function (event) {
                        hoverChecks.push(event.currentTarget === this, event.type === type,
                            event.target === this, event.clientX === 12.5, event.clientY === 20,
                            event.button === -1, event.buttons === 1, event.pointerId === 1,
                            event.bubbles === false, event.cancelable === false);
                        hoverLog.push(`${type}:${name}:${event.relatedTarget?.testName ?? '-'}`);
                        event.preventDefault();
                        hoverChecks.push(event.defaultPrevented === false);
                    });
                }
            }
        "#,
                )
                .await
                .unwrap();
            let (p, q, a, b, revision) = {
                let state = state.borrow();
                let w = state.dom.children(state.dom.root()).unwrap()[0];
                let parents = state.dom.children(w).unwrap();
                let children = state.dom.children(parents[0]).unwrap();
                (
                    parents[0],
                    parents[1],
                    children[0],
                    children[1],
                    state.dom.revision(),
                )
            };
            let position = PhysicalPosition::new(25.0, 40.0);
            for (target, expected) in [
                (
                    Some(a),
                    vec![
                        "pointerenter:app:-",
                        "pointerenter:w:-",
                        "pointerenter:p:-",
                        "pointerenter:a:-",
                    ],
                ),
                (Some(a), vec![]), // Same target after movement or a repaint.
                (Some(b), vec!["pointerleave:a:b", "pointerenter:b:a"]),
                (Some(p), vec!["pointerleave:b:p"]),
                (Some(a), vec!["pointerenter:a:p"]),
                (
                    None,
                    vec![
                        "pointerleave:a:-",
                        "pointerleave:p:-",
                        "pointerleave:w:-",
                        "pointerleave:app:-",
                    ],
                ),
                (None, vec![]), // Repeated native exit is a no-op.
                (
                    Some(b),
                    vec![
                        "pointerenter:app:-",
                        "pointerenter:w:-",
                        "pointerenter:p:-",
                        "pointerenter:b:-",
                    ],
                ),
            ] {
                runtime.eval::<()>("hoverLog = []").await.unwrap();
                queue_test_mouse(&mut host, target, position, 1, (revision, 2.0), None).unwrap();
                let log: Vec<String> = runtime.eval("hoverLog").await.unwrap();
                assert_eq!(log, expected);
                assert!(runtime
                    .eval::<bool>("hoverChecks.every(Boolean)")
                    .await
                    .unwrap());
            }
            // Keep the same leaf hovered but change its ancestry.
            runtime
                .eval::<()>("q.appendChild(b); hoverLog = []")
                .await
                .unwrap();
            queue_test_mouse(&mut host, Some(b), position, 1, (revision, 2.0), None).unwrap();
            assert_eq!(
                runtime.eval::<Vec<String>>("hoverLog").await.unwrap(),
                ["pointerleave:p:b", "pointerenter:q:b"]
            );

            // A queued transition must skip disconnected targets, not their live ancestors.
            queue_test_mouse(&mut host, None, position, 1, (revision, 2.0), None).unwrap();
            runtime.eval::<()>("hoverLog = []").await.unwrap();
            queue_test_mouse(&mut host, Some(b), position, 1, (revision, 2.0), None).unwrap();
            runtime
                .eval::<()>("q.removeChild(b); hoverLog = []")
                .await
                .unwrap();
            queue_test_mouse(&mut host, Some(q), position, 1, (revision, 2.0), None).unwrap();
            assert!(runtime.eval::<bool>("hoverLog.length === 0").await.unwrap());
            runtime
                .eval::<()>("w.removeChild(q); hoverLog = []")
                .await
                .unwrap();
            queue_test_mouse(&mut host, None, position, 1, (revision, 2.0), None).unwrap();
            assert_eq!(
                runtime.eval::<Vec<String>>("hoverLog").await.unwrap(),
                ["pointerleave:w:-", "pointerleave:app:-"]
            );
            assert!(runtime
                .eval::<bool>("hoverChecks.every(Boolean)")
                .await
                .unwrap());

            // Invalid input and rejected queues must not advance the hover path.
            queue_test_mouse(
                &mut host,
                Some(a),
                PhysicalPosition::new(f64::NAN, 0.0),
                1,
                (revision, 2.0),
                None,
            )
            .unwrap();
            assert!(state.borrow().hover_path().is_empty());
            assert!(matches!(
                queue_test_mouse(&mut host, Some(a), position, 1, (revision, 0.0), None),
                Err(HostError::InvalidScaleFactor(0.0))
            ));
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
            queue_test_mouse(&mut host, Some(a), position, 1, (revision, 2.0), None).unwrap();
            assert!(state.borrow().hover_path().is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn release_without_a_presented_frame_cancels_pointer_capture() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (plugin, state) = crate::plugins::dom::DomPlugin::new_with_bindings();
            let (runtime, driver) = runtime::Runtime::builder()
                .plugin(plugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let mut host = ApplicationHost::new(
                state.clone(),
                TextEngine::without_system_fonts(),
                RuntimeLifecycle::for_test(),
            );
            runtime
                .eval::<()>(
                    r#"
                    globalThis.captureWindow = app.createElement('window');
                    globalThis.captureTarget = app.createElement('div');
                    captureWindow.appendChild(captureTarget);
                    app.appendChild(captureWindow);
                    globalThis.terminalLog = [];
                    captureTarget.addEventListener('pointerdown', event =>
                        captureTarget.setPointerCapture(event.pointerId));
                    captureTarget.addEventListener('pointercancel', () =>
                        terminalLog.push('cancel'));
                    captureTarget.addEventListener('lostpointercapture', () =>
                        terminalLog.push('lost'));
                    "#,
                )
                .await
                .unwrap();
            let (target, revision) = {
                let state = state.borrow();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                (state.dom.children(window).unwrap()[0], state.dom.revision())
            };
            let position = PhysicalPosition::new(10.0, 10.0);
            queue_test_mouse(
                &mut host,
                Some(target),
                position,
                1,
                (revision, 1.0),
                Some("pointerdown"),
            )
            .unwrap();
            assert!(runtime
                .eval::<bool>("captureTarget.hasPointerCapture(1)")
                .await
                .unwrap());

            host.queue_mouse_input(
                position,
                0,
                Some((ElementState::Released, MouseButton::Left)),
            )
            .unwrap();

            assert_eq!(
                runtime.eval::<Vec<String>>("terminalLog").await.unwrap(),
                ["cancel", "lost"]
            );
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn full_task_queue_does_not_drop_pointer_cancellation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (plugin, state) = crate::plugins::dom::DomPlugin::new_with_bindings();
            let (runtime, driver) = runtime::Runtime::builder()
                .macrotask_capacity(1)
                .plugin(plugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let mut host = ApplicationHost::new(
                state.clone(),
                TextEngine::without_system_fonts(),
                RuntimeLifecycle::for_test(),
            );
            runtime
                .eval::<()>(
                    r#"
                    globalThis.captureWindow = app.createElement('window');
                    globalThis.captureTarget = app.createElement('div');
                    captureWindow.appendChild(captureTarget);
                    app.appendChild(captureWindow);
                    globalThis.terminalLog = [];
                    captureTarget.addEventListener('pointerdown', event =>
                        captureTarget.setPointerCapture(event.pointerId));
                    captureTarget.addEventListener('pointercancel', () =>
                        terminalLog.push('cancel'));
                    captureTarget.addEventListener('lostpointercapture', () =>
                        terminalLog.push('lost'));
                    "#,
                )
                .await
                .unwrap();
            let (target, revision) = {
                let state = state.borrow();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                (state.dom.children(window).unwrap()[0], state.dom.revision())
            };
            let position = PhysicalPosition::new(10.0, 10.0);
            queue_test_mouse(
                &mut host,
                Some(target),
                position,
                1,
                (revision, 1.0),
                Some("pointerdown"),
            )
            .unwrap();
            assert!(runtime
                .eval::<bool>("captureTarget.hasPointerCapture(1)")
                .await
                .unwrap());

            runtime.macrotask_queue().try_enqueue(|_| Ok(())).unwrap();
            host.queue_pointer_cancel().unwrap();
            tokio::task::yield_now().await;

            assert_eq!(
                runtime.eval::<Vec<String>>("terminalLog").await.unwrap(),
                ["cancel", "lost"]
            );
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pointer_capture_uses_dispatch_time_state_and_reconciles_boundaries() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (plugin, state) = crate::plugins::dom::DomPlugin::new_with_bindings();
            let (runtime, driver) = runtime::Runtime::builder()
                .plugin(plugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let mut host = ApplicationHost::new(
                state.clone(),
                TextEngine::without_system_fonts(),
                RuntimeLifecycle::for_test(),
            );
            runtime
                .eval::<()>(
                    r#"
                    globalThis.captureWindow = app.createElement('window');
                    globalThis.captureA = app.createElement('div');
                    globalThis.captureB = app.createElement('div');
                    captureWindow.appendChild(captureA);
                    captureWindow.appendChild(captureB);
                    app.appendChild(captureWindow);
                    globalThis.pointerBoundaryLog = [];
                    captureA.addEventListener('pointerdown', event =>
                        captureA.setPointerCapture(event.pointerId));
                    captureA.addEventListener('pointermove', () =>
                        pointerBoundaryLog.push('move:a'));
                    captureA.addEventListener('pointerleave', () =>
                        pointerBoundaryLog.push('leave:a'));
                    captureB.addEventListener('pointerenter', () =>
                        pointerBoundaryLog.push('enter:b'));
                    captureA.addEventListener('pointerup', () =>
                        pointerBoundaryLog.push('up:a'));
                    captureA.addEventListener('lostpointercapture', () =>
                        pointerBoundaryLog.push('lost:a'));
                    captureA.addEventListener('click', () =>
                        pointerBoundaryLog.push('click:a'));
                    captureB.addEventListener('pointerup', () =>
                        pointerBoundaryLog.push('up:b'));
                    captureB.addEventListener('pointermove', () =>
                        pointerBoundaryLog.push('move:b'));
                    "#,
                )
                .await
                .unwrap();
            let (a, b, revision) = {
                let state = state.borrow();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                let children = state.dom.children(window).unwrap();
                (children[0], children[1], state.dom.revision())
            };
            let position = PhysicalPosition::new(10.0, 10.0);
            queue_test_mouse(
                &mut host,
                Some(a),
                position,
                1,
                (revision, 1.0),
                Some("pointerdown"),
            )
            .unwrap();
            // Queue movement outside every hit region before pointerdown JavaScript runs.
            queue_test_mouse(
                &mut host,
                None,
                position,
                1,
                (revision, 1.0),
                Some("pointermove"),
            )
            .unwrap();
            assert_eq!(
                runtime
                    .eval::<Vec<String>>("pointerBoundaryLog")
                    .await
                    .unwrap(),
                ["move:a"]
            );
            assert_eq!(state.borrow().pointer_capture_target(), Some(a));
            runtime.eval::<()>("pointerBoundaryLog = []").await.unwrap();

            queue_test_mouse(
                &mut host,
                Some(b),
                position,
                0,
                (revision, 1.0),
                Some("pointerup"),
            )
            .unwrap();
            assert_eq!(
                runtime
                    .eval::<Vec<String>>("pointerBoundaryLog")
                    .await
                    .unwrap(),
                vec!["up:a", "lost:a", "click:a"]
            );
            assert_eq!(state.borrow().pointer_capture_target(), None);
            runtime.eval::<()>("pointerBoundaryLog = []").await.unwrap();

            queue_test_mouse(&mut host, Some(b), position, 0, (revision, 1.0), None).unwrap();
            assert_eq!(
                runtime
                    .eval::<Vec<String>>("pointerBoundaryLog")
                    .await
                    .unwrap(),
                ["leave:a", "enter:b"]
            );

            // A capture target detached before dispatch must not retain routing authority.
            queue_test_mouse(
                &mut host,
                Some(a),
                position,
                1,
                (revision, 1.0),
                Some("pointerdown"),
            )
            .unwrap();
            assert!(runtime
                .eval::<bool>("captureA.hasPointerCapture(1)")
                .await
                .unwrap());
            runtime
                .eval::<()>("captureWindow.removeChild(captureA); pointerBoundaryLog = []")
                .await
                .unwrap();
            queue_test_mouse(
                &mut host,
                Some(b),
                position,
                1,
                (revision, 1.0),
                Some("pointermove"),
            )
            .unwrap();
            assert_eq!(
                runtime
                    .eval::<Vec<String>>("pointerBoundaryLog")
                    .await
                    .unwrap(),
                ["enter:b", "move:b"]
            );
            assert_eq!(state.borrow().pointer_capture_target(), None);

            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pointer_input_reaches_dom_in_order_with_presented_coordinates() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (plugin, state) = crate::plugins::dom::DomPlugin::new_with_bindings();
            let (runtime, driver) = runtime::Runtime::builder()
                .plugin(plugin)
                .build_driven()
                .await
                .unwrap();
            let driver = tokio::task::spawn_local(driver.run());
            let mut host = ApplicationHost::new(
                state.clone(),
                TextEngine::without_system_fonts(),
                RuntimeLifecycle::for_test(),
            );
            runtime
                .eval::<()>(
                    r#"
            globalThis.mouseWindow = app.createElement('window');
            globalThis.mouseTarget = app.createElement('div');
            mouseTarget.style.setProperty('width', '100px');
            mouseTarget.style.setProperty('height', '100px');
            mouseWindow.appendChild(mouseTarget);
            app.appendChild(mouseWindow);
            globalThis.inputLog = [];
            mouseTarget.addEventListener('pointerenter', () => inputLog.push('enter'));
            for (const type of ['pointerdown', 'pointerup', 'pointermove', 'click']) {
                mouseTarget.addEventListener(type, event => {
                    inputLog.push(`${event.type}:${event.button}:${event.buttons}`);
                    event.preventDefault();
                });
                mouseWindow.addEventListener(type, () => inputLog.push('bubble'));
            }
        "#,
                )
                .await
                .unwrap();
            let (plan, target, window) = {
                let state = state.borrow();
                let window = state.dom.children(state.dom.root()).unwrap()[0];
                let target = state.dom.children(window).unwrap()[0];
                let mut layout = LayoutEngine::new(TextEngine::without_system_fonts());
                let computed = layout
                    .compute(&state.dom, LogicalViewport::new(320.0, 240.0).unwrap())
                    .unwrap();
                (
                    ScenePlan::from_layout(&state.dom, computed, PhysicalSize::new(640, 480), 2.0)
                        .unwrap(),
                    target,
                    window,
                )
            };
            let position = PhysicalPosition::new(25.0, 20.0);
            for (input, buttons, expected_kind) in [
                (
                    Some((ElementState::Pressed, MouseButton::Left)),
                    1,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::PRIMARY,
                        pressed: true,
                    },
                ),
                (None, 1, NativeMouseInputKind::Move),
                (
                    Some((ElementState::Pressed, MouseButton::Right)),
                    3,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::SECONDARY,
                        pressed: true,
                    },
                ),
                (
                    Some((ElementState::Released, MouseButton::Right)),
                    1,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::SECONDARY,
                        pressed: false,
                    },
                ),
                (
                    Some((ElementState::Released, MouseButton::Left)),
                    0,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::PRIMARY,
                        pressed: false,
                    },
                ),
                (None, 0, NativeMouseInputKind::Move),
                (
                    Some((ElementState::Pressed, MouseButton::Middle)),
                    4,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::AUXILIARY,
                        pressed: true,
                    },
                ),
                (
                    Some((ElementState::Released, MouseButton::Middle)),
                    0,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::AUXILIARY,
                        pressed: false,
                    },
                ),
                (
                    Some((ElementState::Pressed, MouseButton::Other(3))),
                    8,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::from_code(3),
                        pressed: true,
                    },
                ),
                (
                    Some((ElementState::Released, MouseButton::Other(3))),
                    0,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::from_code(3),
                        pressed: false,
                    },
                ),
                (
                    Some((ElementState::Pressed, MouseButton::Other(4))),
                    16,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::from_code(4),
                        pressed: true,
                    },
                ),
                (
                    Some((ElementState::Released, MouseButton::Other(4))),
                    0,
                    NativeMouseInputKind::Button {
                        button: ChangedMouseButton::from_code(4),
                        pressed: false,
                    },
                ),
            ] {
                let native = pointer_input_for_input(&plan, position, buttons, input);
                assert_eq!(native.hit_target, Some(target));
                assert_eq!(native.kind, expected_kind);
                assert_eq!(native.presented_revision, plan.revision());
                assert_eq!((native.client_x, native.client_y), (12.5, 10.0));
                assert_eq!(native.buttons.bits(), buttons);
                host.queue_pointer_input(native, plan.scale_factor())
                    .unwrap();
            }
            let log: Vec<String> = runtime.eval("inputLog").await.unwrap();
            let expected: Vec<_> = std::iter::once("enter")
                .chain(
                    [
                        "pointerdown:0:1",
                        "pointermove:-1:1",
                        "pointermove:2:3",
                        "pointermove:2:1",
                        "pointerup:0:0",
                        "click:0:0",
                        "pointermove:-1:0",
                        "pointerdown:1:4",
                        "pointerup:1:0",
                        "pointerdown:3:8",
                        "pointerup:3:0",
                        "pointerdown:4:16",
                        "pointerup:4:0",
                    ]
                    .into_iter()
                    .flat_map(|event| [event, "bubble"]),
                )
                .collect();
            assert_eq!(log, expected);

            let precise_wheel = wheel_input_for_input(&plan, position, 8.0, -4.0, true, 1);
            assert_eq!(precise_wheel.hit_target, Some(target));
            assert_eq!(
                precise_wheel.kind,
                NativeMouseInputKind::Wheel {
                    delta_x: -4.0,
                    delta_y: 2.0,
                    delta_mode: WheelDeltaMode::Pixel,
                }
            );
            let line_wheel = wheel_input_for_input(&plan, position, 3.0, -5.0, false, 0);
            assert_eq!(
                line_wheel.kind,
                NativeMouseInputKind::Wheel {
                    delta_x: -3.0,
                    delta_y: 5.0,
                    delta_mode: WheelDeltaMode::Line,
                }
            );
            let outside = PhysicalPosition::new(-1.0, -1.0);
            let outside_wheel = wheel_input_for_input(&plan, outside, 1.0, 1.0, true, 0);
            assert_eq!(outside_wheel.hit_target, None);
            assert!(matches!(
                outside_wheel.kind,
                NativeMouseInputKind::Wheel { .. }
            ));

            let up = Some((ElementState::Released, MouseButton::Left));
            let outside_up = pointer_input_for_input(&plan, outside, 0, up);
            assert_eq!(outside_up.hit_target, None);
            assert_eq!(
                outside_up.kind,
                NativeMouseInputKind::Button {
                    button: ChangedMouseButton::PRIMARY,
                    pressed: false,
                }
            );
            let outside_move = pointer_input_for_input(&plan, outside, 1, None);
            assert_eq!(outside_move.hit_target, None);
            assert_eq!(outside_move.kind, NativeMouseInputKind::Move);

            let window_up =
                pointer_input_for_input(&plan, PhysicalPosition::new(400.0, 400.0), 0, up);
            assert_eq!(window_up.hit_target, Some(window));
            runtime.shutdown().await.unwrap();
            driver.await.unwrap();
        })
        .await;
}

#[test]
fn chorded_button_transitions_queue_changed_button_facts() {
    let plan = scene_plan(&Dom::new());
    let position = PhysicalPosition::new(10.0, 10.0);

    for (state, buttons) in [(ElementState::Pressed, 3), (ElementState::Released, 1)] {
        let input =
            pointer_input_for_input(&plan, position, buttons, Some((state, MouseButton::Right)));
        assert_eq!(
            input.kind,
            NativeMouseInputKind::Button {
                button: ChangedMouseButton::SECONDARY,
                pressed: state == ElementState::Pressed,
            }
        );
        assert_eq!(input.buttons.bits(), buttons);
    }
}

#[test]
fn stale_success_and_error_are_discarded_before_installation() {
    for status in [PendingWindowStatus::Removed, PendingWindowStatus::Replaced] {
        assert_eq!(
            accept_current_graphics_result(status, Ok::<_, &'static str>(7_u8)),
            None
        );
        assert_eq!(
            accept_current_graphics_result(status, Err::<u8, _>("stale error")),
            None
        );
    }

    assert_eq!(
        accept_current_graphics_result(PendingWindowStatus::Current, Ok::<_, &'static str>(7_u8)),
        Some(Ok(7))
    );
    assert!(!graphics_initialization_stop_is_fatal(
        PendingWindowStatus::Removed
    ));
    assert!(!graphics_initialization_stop_is_fatal(
        PendingWindowStatus::Replaced
    ));
    assert!(graphics_initialization_stop_is_fatal(
        PendingWindowStatus::Current
    ));
}

#[test]
fn incompatible_replacement_surface_selects_another_adapter() {
    let adapter_a = 1_u8;
    let adapter_b = 2_u8;

    assert!(matches!(
        classify_replacement_renderer(Ok::<_, GraphicsError>(adapter_a)),
        Ok(ReplacementRenderer::Reuse(selected)) if selected == adapter_a
    ));
    let selected_for_surface_b =
        match classify_replacement_renderer(Err::<u8, _>(GraphicsError::UnsupportedSurface)) {
            Ok(ReplacementRenderer::SelectCompatibleAdapter) => adapter_b,
            other => panic!("surface B should reselect its adapter, got {other:?}"),
        };

    assert_eq!(selected_for_surface_b, adapter_b);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_stalled_graphics_initialization_aborts_its_task() {
    let task = tokio::spawn(std::future::pending());
    let abort = task.abort_handle();
    let task = AbortOnDrop::new(task);

    tokio::task::yield_now().await;
    drop(task);
    tokio::task::yield_now().await;

    assert!(abort.is_finished());
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_graphics_drops_surface_before_candidate_window() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = oneshot::channel::<DropProbe>();
    let task_events = Arc::clone(&events);
    let task = tokio::spawn(async move {
        let _surface = drop_probe("surface", &task_events);
        let _sender = sender;
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;

    cancel_graphics_task(task, receiver, drop_probe("window", &events)).await;
    assert_eq!(*events.lock().unwrap(), ["surface", "window"]);

    events.lock().unwrap().clear();
    let (sender, receiver) = oneshot::channel();
    let (ready_sender, ready_receiver) = oneshot::channel();
    let task_events = Arc::clone(&events);
    let task = tokio::spawn(async move {
        let _ = sender.send(drop_probe("surface", &task_events));
        let _ = ready_sender.send(());
        std::future::pending::<()>().await;
    });
    ready_receiver.await.unwrap();

    cancel_graphics_task(task, receiver, drop_probe("window", &events)).await;
    assert_eq!(*events.lock().unwrap(), ["surface", "window"]);
}

#[test]
fn physical_pixels_convert_to_logical_viewport_once() {
    let viewport = logical_viewport(PhysicalSize::new(1600, 1200), 2.0).unwrap();
    assert_eq!(viewport.width(), 800.0);
    assert_eq!(viewport.height(), 600.0);
}

#[test]
fn invalid_scale_factors_are_rejected() {
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            logical_viewport(PhysicalSize::new(800, 600), scale),
            Err(HostError::InvalidScaleFactor(_))
        ));
    }
}

#[test]
fn candidate_failures_recover_only_after_a_frame_was_presented() {
    for kind in [
        FailureKind::WindowSync,
        FailureKind::TargetTooLarge,
        FailureKind::Layout,
        FailureKind::Scene,
    ] {
        assert_eq!(failure_policy(false, kind), FailurePolicy::Fatal);
        assert_eq!(failure_policy(true, kind), FailurePolicy::Recoverable);
    }
}

#[test]
fn active_presentation_and_invariant_failures_remain_fatal() {
    for kind in [FailureKind::ActivePresentation, FailureKind::Invariant] {
        assert_eq!(failure_policy(false, kind), FailurePolicy::Fatal);
        assert_eq!(failure_policy(true, kind), FailurePolicy::Fatal);
    }

    assert!(matches!(
        classify_fatal_failure(
            true,
            FailureKind::ActivePresentation,
            HostError::Graphics(GraphicsError::SurfaceValidation),
        ),
        RedrawFailure::Fatal(HostError::Graphics(GraphicsError::SurfaceValidation))
    ));
}

#[test]
fn oversized_resize_is_recoverable_only_after_the_active_renderer_presented() {
    assert!(matches!(
        classify_resize_failure(42, false, oversized_target()),
        RedrawFailure::Fatal(HostError::Graphics(GraphicsError::TargetTooLarge { .. }))
    ));

    let failure = classify_resize_failure(42, true, oversized_target());
    assert!(matches!(
        failure,
        RedrawFailure::Recoverable(FrameFailure {
            revision: 42,
            stage: FrameStage::Resize,
            ..
        })
    ));
}

#[test]
fn oversized_native_size_keeps_renderer_recovery_state_without_a_usable_frame() {
    let mut dom = Dom::new();
    let window = dom.create_element(Element::from_tag(ElementTag::Window));
    dom.append_child(dom.root(), window).unwrap();
    let plan = scene_plan(&dom);
    let revision = plan.revision();
    let surface = test_surface(1, PhysicalSize::new(320, 240), 1);
    let presented = PresentedFrame { plan, surface };

    let matching = presentation_state(Some(&presented), Some(&surface), Some(revision));
    assert_eq!(
        matching,
        PresentationState {
            active_renderer_has_presented: true,
            usable_frame: true,
        }
    );

    // `WindowRenderer::resize` rejects the oversized native size before
    // changing this retained surface or its presentation history.
    let oversized = presentation_state(Some(&presented), None, Some(revision));
    assert_eq!(
        oversized,
        PresentationState {
            active_renderer_has_presented: true,
            usable_frame: false,
        }
    );
    assert!(matches!(
        classify_resize_failure(
            revision,
            oversized.active_renderer_has_presented,
            oversized_target(),
        ),
        RedrawFailure::Recoverable(FrameFailure {
            stage: FrameStage::Resize,
            ..
        })
    ));
}

#[test]
fn valid_presentation_recovers_after_oversized_then_supported_resize() {
    let mut dom = Dom::new();
    let window = dom.create_element(Element::from_tag(ElementTag::Window));
    dom.append_child(dom.root(), window).unwrap();
    let initial_plan = scene_plan(&dom);
    let revision = initial_plan.revision();
    let retained_surface = test_surface(1, PhysicalSize::new(320, 240), 1);
    let mut presented = None;
    let mut last_failure = None;
    finish_successful_presentation(
        &mut presented,
        &mut last_failure,
        initial_plan,
        retained_surface,
    );

    let oversized = presentation_state(presented.as_ref(), None, Some(revision));
    assert!(oversized.active_renderer_has_presented);
    assert!(!oversized.usable_frame);
    let failure = classify_resize_failure(
        revision,
        oversized.active_renderer_has_presented,
        oversized_target(),
    );
    let RedrawFailure::Recoverable(failure) = failure else {
        panic!("an oversized resize after presentation must be recoverable");
    };

    // The host suppresses stale hit testing while the rejected resize
    // leaves the renderer's last valid surface available for recovery.
    presented = None;
    last_failure = Some(failure);
    let supported = presentation_state(presented.as_ref(), Some(&retained_surface), Some(revision));
    assert!(supported.active_renderer_has_presented);
    assert!(!supported.usable_frame);

    let recovered_plan = scene_plan(&dom);
    finish_successful_presentation(
        &mut presented,
        &mut last_failure,
        recovered_plan,
        retained_surface,
    );
    let recovered = presentation_state(presented.as_ref(), Some(&retained_surface), Some(revision));
    assert!(recovered.usable_frame);
    assert!(last_failure.is_none());
}

#[test]
fn successful_presentation_advances_the_plan_and_clears_failure() {
    let mut dom = Dom::new();
    let window = dom.create_element(Element::from_tag(ElementTag::Window));
    dom.append_child(dom.root(), window).unwrap();
    let first_plan = scene_plan(&dom);
    let first_revision = first_plan.revision();

    dom.set_attribute(window, "title".into(), "updated".into())
        .unwrap();
    let second_plan = scene_plan(&dom);
    let second_revision = second_plan.revision();
    assert!(second_revision > first_revision);

    let mut presented = None;
    let mut last_failure = None;
    finish_successful_presentation(
        &mut presented,
        &mut last_failure,
        first_plan,
        test_surface(1, PhysicalSize::new(320, 240), 1),
    );
    last_failure = Some(FrameFailure {
        revision: second_revision,
        stage: FrameStage::Scene,
        message: "injected scene failure".into(),
    });

    finish_successful_presentation(
        &mut presented,
        &mut last_failure,
        second_plan,
        test_surface(1, PhysicalSize::new(320, 240), 1),
    );

    assert_eq!(
        presented.as_ref().map(PresentedFrame::revision),
        Some(second_revision)
    );
    assert!(last_failure.is_none());
}

#[test]
fn replacement_first_frame_failure_cannot_recover_from_the_old_window() {
    let mut dom = Dom::new();
    let window = dom.create_element(Element::from_tag(ElementTag::Window));
    dom.append_child(dom.root(), window).unwrap();
    let plan = scene_plan(&dom);
    let revision = plan.revision();
    let old_surface = test_surface(1, PhysicalSize::new(320, 240), 7);
    let replacement_surface = test_surface(2, PhysicalSize::new(320, 240), 8);
    let presented = PresentedFrame {
        plan,
        surface: old_surface,
    };

    assert!(presented_frame_is_usable(
        Some(&presented),
        Some(&old_surface),
        Some(revision),
    ));
    assert!(!presented_frame_is_usable(
        Some(&presented),
        Some(&replacement_surface),
        Some(revision),
    ));
    let has_presented_frame =
        presented_frame_is_usable(Some(&presented), Some(&replacement_surface), None);
    assert!(!has_presented_frame);
    assert!(matches!(
        classify_candidate_failure(
            revision + 1,
            has_presented_frame,
            FailureKind::Layout,
            FrameStage::Layout,
            HostError::MissingRenderer,
        ),
        RedrawFailure::Fatal(HostError::MissingRenderer)
    ));
}

#[test]
fn reconfiguration_then_candidate_failure_cannot_recover_from_old_pixels() {
    let mut dom = Dom::new();
    let window = dom.create_element(Element::from_tag(ElementTag::Window));
    dom.append_child(dom.root(), window).unwrap();
    let plan = scene_plan(&dom);
    let revision = plan.revision();
    let old_surface = test_surface(1, PhysicalSize::new(320, 240), 11);
    let reconfigured_surface = test_surface(1, PhysicalSize::new(320, 240), 12);
    let presented = PresentedFrame {
        plan,
        surface: old_surface,
    };

    assert!(presented_frame_is_usable(
        Some(&presented),
        Some(&old_surface),
        Some(revision),
    ));
    let state = presentation_state(Some(&presented), Some(&reconfigured_surface), None);
    assert_eq!(
        state,
        PresentationState {
            active_renderer_has_presented: false,
            usable_frame: false,
        }
    );
    assert!(matches!(
        classify_candidate_failure(
            revision + 1,
            state.usable_frame,
            FailureKind::Scene,
            FrameStage::Scene,
            HostError::MissingRenderer,
        ),
        RedrawFailure::Fatal(HostError::MissingRenderer)
    ));
}

#[test]
fn resize_then_failure_cannot_recover_from_the_old_surface() {
    let mut dom = Dom::new();
    let window = dom.create_element(Element::from_tag(ElementTag::Window));
    dom.append_child(dom.root(), window).unwrap();
    let plan = scene_plan(&dom);
    let revision = plan.revision();
    let old_surface = test_surface(1, PhysicalSize::new(320, 240), 11);
    let resized_surface = test_surface(1, PhysicalSize::new(640, 480), 12);
    let presented = PresentedFrame {
        plan,
        surface: old_surface,
    };

    assert!(presented_frame_is_usable(
        Some(&presented),
        Some(&old_surface),
        Some(revision),
    ));
    assert!(!presented_frame_is_usable(
        Some(&presented),
        Some(&resized_surface),
        Some(revision),
    ));
    let has_presented_frame =
        presented_frame_is_usable(Some(&presented), Some(&resized_surface), None);
    assert!(!has_presented_frame);
    assert!(matches!(
        classify_candidate_failure(
            revision + 1,
            has_presented_frame,
            FailureKind::Scene,
            FrameStage::Scene,
            HostError::MissingRenderer,
        ),
        RedrawFailure::Fatal(HostError::MissingRenderer)
    ));
}
