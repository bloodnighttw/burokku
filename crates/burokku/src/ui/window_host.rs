//! Native ownership for the single committed `<window>` element.

use std::sync::Arc;

use thiserror::Error;
use winit::{ActiveEventLoop, LogicalSize, Window, WindowAttributes, WindowId};

use super::elements::{styles::window::WindowSize, Element, NodeId, NodeKind, PublishedDom};

const DEFAULT_WINDOW_WIDTH: f64 = 800.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 600.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowSpec {
    dom_id: NodeId,
    title: String,
    inner_size: LogicalSize<f64>,
}

impl WindowSpec {
    pub(crate) fn from_publication(
        publication: &PublishedDom,
    ) -> Result<Option<Self>, WindowHostError> {
        let snapshot = publication.snapshot();
        let app = snapshot.root();
        if !matches!(snapshot.kind(app), Some(NodeKind::App)) {
            return Err(WindowHostError::InvalidAppRoot);
        }
        let children = snapshot
            .children(app)
            .ok_or(WindowHostError::InvalidAppRoot)?;
        let Some(&dom_id) = children.first() else {
            return Ok(None);
        };
        if children.len() != 1 || snapshot.parent(dom_id) != Some(app) {
            return Err(WindowHostError::InvalidAppChildren(children.len()));
        }
        let Some(Element::Window { style }) = snapshot.element(dom_id) else {
            return Err(WindowHostError::ExpectedWindow(dom_id));
        };

        let width = requested_dimension(style.width, DEFAULT_WINDOW_WIDTH);
        let height = requested_dimension(style.height, DEFAULT_WINDOW_HEIGHT);
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            return Err(WindowHostError::InvalidInitialSize {
                node: dom_id,
                width,
                height,
            });
        }

        Ok(Some(Self {
            dom_id,
            title: snapshot
                .attribute(dom_id, "title")
                .unwrap_or("Burokku")
                .to_owned(),
            inner_size: LogicalSize::new(width, height),
        }))
    }

    #[cfg(test)]
    pub(crate) fn dom_id(&self) -> NodeId {
        self.dom_id
    }

    #[cfg(test)]
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    #[cfg(test)]
    pub(crate) fn inner_size(&self) -> LogicalSize<f64> {
        self.inner_size
    }

    pub(crate) fn attributes(&self) -> WindowAttributes {
        Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(self.inner_size)
    }
}

fn requested_dimension(value: WindowSize, default: f64) -> f64 {
    match value {
        WindowSize::Auto => default,
        WindowSize::Fixed(value) => f64::from(value),
        // Until monitor/work-area APIs exist, percentages are relative to the
        // documented default initial content size rather than an implicit CSS
        // containing block.
        WindowSize::Percent(value) => default * f64::from(value),
    }
}

#[derive(Debug)]
pub(crate) enum WindowChange {
    Unchanged,
    Created,
    Updated,
    PreparedReplacement(PreparedWindow),
    Removed,
}

#[derive(Debug)]
pub(crate) struct NativeWindow {
    spec: WindowSpec,
    window: Arc<Window>,
}

impl NativeWindow {
    pub(crate) fn id(&self) -> WindowId {
        self.window.id()
    }

    pub(crate) fn window(&self) -> &Arc<Window> {
        &self.window
    }

    pub(crate) fn close(self) {
        self.window.close();
    }
}

#[derive(Debug)]
struct PreparedReplacement<T> {
    candidate: Option<T>,
    abort: fn(&T),
}

impl<T> PreparedReplacement<T> {
    fn new(candidate: T, abort: fn(&T)) -> Self {
        Self {
            candidate: Some(candidate),
            abort,
        }
    }

    fn candidate(&self) -> &T {
        self.candidate
            .as_ref()
            .expect("an uncommitted replacement retains its candidate")
    }

    fn commit(mut self, current: &mut Option<T>) -> Option<T> {
        let candidate = self
            .candidate
            .take()
            .expect("an uncommitted replacement retains its candidate");
        current.replace(candidate)
    }

    fn commit_with_dependent<R>(
        self,
        current: &mut Option<T>,
        active_dependent: &mut Option<R>,
        candidate_dependent: R,
    ) -> (Option<T>, Option<R>) {
        // Install the already-prepared dependent resource first. Committing
        // the candidate itself is infallible, so no failure can leave the
        // active Window without its matching dependent resource.
        let previous_dependent = active_dependent.replace(candidate_dependent);
        let previous = self.commit(current);
        (previous, previous_dependent)
    }
}

impl<T> Drop for PreparedReplacement<T> {
    fn drop(&mut self) {
        if let Some(candidate) = self.candidate.as_ref() {
            (self.abort)(candidate);
        }
    }
}

#[derive(Debug)]
pub(crate) struct PreparedWindow {
    replacement: PreparedReplacement<NativeWindow>,
}

impl PreparedWindow {
    fn new(candidate: NativeWindow) -> Self {
        Self {
            replacement: PreparedReplacement::new(candidate, |candidate| {
                candidate.window.close();
            }),
        }
    }

    pub(crate) fn window(&self) -> &Arc<Window> {
        self.replacement.candidate().window()
    }

    pub(crate) fn commit_with<R>(
        self,
        manager: &mut WindowManager,
        active_dependent: &mut Option<R>,
        candidate_dependent: R,
    ) -> (Option<NativeWindow>, Option<R>) {
        self.replacement.commit_with_dependent(
            &mut manager.current,
            active_dependent,
            candidate_dependent,
        )
    }
}

#[derive(Debug, Default)]
pub(crate) struct WindowManager {
    current: Option<NativeWindow>,
}

impl WindowManager {
    pub(crate) fn current(&self) -> Option<&NativeWindow> {
        self.current.as_ref()
    }

    pub(crate) fn reconcile(
        &mut self,
        event_loop: &ActiveEventLoop,
        publication: &PublishedDom,
    ) -> Result<WindowChange, WindowHostError> {
        let desired = WindowSpec::from_publication(publication)?;
        match (self.current.as_mut(), desired) {
            (None, None) => Ok(WindowChange::Unchanged),
            (Some(current), None) => {
                current.window.close();
                self.current = None;
                Ok(WindowChange::Removed)
            }
            (None, Some(spec)) => {
                let window = Arc::new(event_loop.create_window(spec.attributes())?);
                self.current = Some(NativeWindow { spec, window });
                Ok(WindowChange::Created)
            }
            (Some(current), Some(spec)) if current.spec.dom_id == spec.dom_id => {
                if current.spec == spec {
                    return Ok(WindowChange::Unchanged);
                }
                let window = &current.window;
                apply_same_window_update(
                    &mut current.spec,
                    spec,
                    |size| window.set_inner_size(size),
                    |title| window.set_title(title),
                    || window.request_redraw(),
                )?;
                Ok(WindowChange::Updated)
            }
            (Some(_), Some(spec)) => {
                // Keep the active native Window untouched until the host also
                // creates a renderer for this candidate. Dropping an
                // uncommitted candidate closes only that candidate.
                let window = Arc::new(event_loop.create_window(spec.attributes())?);
                Ok(WindowChange::PreparedReplacement(PreparedWindow::new(
                    NativeWindow { spec, window },
                )))
            }
        }
    }

    pub(crate) fn close(&mut self) {
        if let Some(current) = self.current.take() {
            current.window.close();
        }
    }
}

fn apply_same_window_update<E>(
    current: &mut WindowSpec,
    desired: WindowSpec,
    mut set_inner_size: impl FnMut(LogicalSize<f64>) -> Result<(), E>,
    mut set_title: impl FnMut(&str),
    mut request_redraw: impl FnMut(),
) -> Result<(), E> {
    debug_assert_eq!(current.dom_id, desired.dom_id);

    // WindowSpec::from_publication validated the complete desired spec before
    // this function is reached. Perform the fallible operation first so a
    // reported size failure cannot leave the title or committed spec changed.
    if current.inner_size != desired.inner_size {
        set_inner_size(desired.inner_size)?;
    }
    if current.title != desired.title {
        set_title(&desired.title);
    }
    *current = desired;
    request_redraw();
    Ok(())
}

impl Drop for WindowManager {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug, Error)]
pub(crate) enum WindowHostError {
    #[error("the committed App root is missing or invalid")]
    InvalidAppRoot,

    #[error("App must contain zero or one Window child, found {0}")]
    InvalidAppChildren(usize),

    #[error("App child {0:?} is not a Window element")]
    ExpectedWindow(NodeId),

    #[error("Window {node:?} requested invalid initial size {width}x{height}")]
    InvalidInitialSize {
        node: NodeId,
        width: f64,
        height: f64,
    },

    #[error(transparent)]
    Native(#[from] winit::Error),
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    use crate::ui::elements::{Dom, DomPublisher, ElementTag};

    use super::*;

    fn publication(dom: &Dom) -> Arc<PublishedDom> {
        let (_publisher, reader) = DomPublisher::new(dom, |_| {});
        reader.load()
    }

    #[derive(Debug)]
    struct ReplacementProbe {
        id: u8,
        closed: Rc<Cell<bool>>,
        events: Rc<RefCell<Vec<String>>>,
    }

    fn close_probe(probe: &ReplacementProbe) {
        probe.closed.set(true);
        probe
            .events
            .borrow_mut()
            .push(format!("window {} closed", probe.id));
    }

    #[derive(Debug)]
    struct DependentProbe {
        id: u8,
        events: Rc<RefCell<Vec<String>>>,
    }

    impl Drop for DependentProbe {
        fn drop(&mut self) {
            self.events
                .borrow_mut()
                .push(format!("dependent {} dropped", self.id));
        }
    }

    fn replacement_probe(
        id: u8,
        closed: &Rc<Cell<bool>>,
        events: &Rc<RefCell<Vec<String>>>,
    ) -> ReplacementProbe {
        ReplacementProbe {
            id,
            closed: Rc::clone(closed),
            events: Rc::clone(events),
        }
    }

    fn window_spec(dom_id: NodeId, title: &str, width: f64, height: f64) -> WindowSpec {
        WindowSpec {
            dom_id,
            title: title.into(),
            inner_size: LogicalSize::new(width, height),
        }
    }

    #[test]
    fn failed_candidate_setup_keeps_active_window_and_dependent() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let active_closed = Rc::new(Cell::new(false));
        let candidate_closed = Rc::new(Cell::new(false));
        let current = Some(replacement_probe(1, &active_closed, &events));
        let active_dependent = Some(10_u8);

        let setup = {
            let _prepared = PreparedReplacement::new(
                replacement_probe(2, &candidate_closed, &events),
                close_probe,
            );
            Err::<u8, _>("injected dependent creation failure")
        };

        assert_eq!(setup, Err("injected dependent creation failure"));
        assert_eq!(current.as_ref().map(|probe| probe.id), Some(1));
        assert_eq!(active_dependent, Some(10));
        assert!(!active_closed.get());
        assert!(candidate_closed.get());
        assert_eq!(&*events.borrow(), &["window 2 closed"]);
    }

    #[test]
    fn successful_handoff_installs_dependent_before_old_window_closes() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let active_closed = Rc::new(Cell::new(false));
        let candidate_closed = Rc::new(Cell::new(false));
        let mut current = Some(replacement_probe(1, &active_closed, &events));
        let mut active_dependent = Some(DependentProbe {
            id: 1,
            events: Rc::clone(&events),
        });
        let prepared = PreparedReplacement::new(
            replacement_probe(2, &candidate_closed, &events),
            close_probe,
        );
        let candidate_dependent = DependentProbe {
            id: 2,
            events: Rc::clone(&events),
        };

        let (previous_window, previous_dependent) = prepared.commit_with_dependent(
            &mut current,
            &mut active_dependent,
            candidate_dependent,
        );

        assert_eq!(current.as_ref().map(|probe| probe.id), Some(2));
        assert_eq!(
            active_dependent.as_ref().map(|dependent| dependent.id),
            Some(2)
        );
        assert!(events.borrow().is_empty());

        drop(previous_dependent);
        let previous_window = previous_window.unwrap();
        close_probe(&previous_window);

        assert_eq!(
            &*events.borrow(),
            &["dependent 1 dropped", "window 1 closed"]
        );
        assert!(active_closed.get());
        assert!(!candidate_closed.get());
    }

    #[test]
    fn same_window_size_failure_keeps_spec_and_title_untouched() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        let original = window_spec(window, "old", 800.0, 600.0);
        let desired = window_spec(window, "new", 1024.0, 768.0);
        let mut current = original.clone();
        let events = Rc::new(RefCell::new(Vec::new()));

        let result = apply_same_window_update(
            &mut current,
            desired,
            {
                let events = Rc::clone(&events);
                move |_| {
                    events.borrow_mut().push("size");
                    Err("injected size failure")
                }
            },
            {
                let events = Rc::clone(&events);
                move |_| events.borrow_mut().push("title")
            },
            {
                let events = Rc::clone(&events);
                move || events.borrow_mut().push("redraw")
            },
        );

        assert_eq!(result, Err("injected size failure"));
        assert_eq!(current, original);
        assert_eq!(&*events.borrow(), &["size"]);
    }

    #[test]
    fn successful_same_window_update_commits_spec_and_requests_redraw() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        let mut current = window_spec(window, "old", 800.0, 600.0);
        let desired = window_spec(window, "new", 1024.0, 768.0);
        let events = Rc::new(RefCell::new(Vec::new()));

        let result: Result<(), ()> = apply_same_window_update(
            &mut current,
            desired.clone(),
            {
                let events = Rc::clone(&events);
                move |_| {
                    events.borrow_mut().push("size");
                    Ok(())
                }
            },
            {
                let events = Rc::clone(&events);
                move |_| events.borrow_mut().push("title")
            },
            {
                let events = Rc::clone(&events);
                move || events.borrow_mut().push("redraw")
            },
        );

        assert_eq!(result, Ok(()));
        assert_eq!(current, desired);
        assert_eq!(&*events.borrow(), &["size", "title", "redraw"]);
    }

    #[test]
    fn window_manager_and_spec_allow_an_initial_windowless_state() {
        let manager = WindowManager::default();
        assert!(manager.current().is_none());

        let dom = Dom::new();
        assert_eq!(
            WindowSpec::from_publication(&publication(&dom)).unwrap(),
            None
        );
    }

    #[test]
    fn fixed_window_styles_and_title_define_initial_attributes() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        dom.set_style_property(window, "width", "640px").unwrap();
        dom.set_style_property(window, "height", "480px").unwrap();
        dom.set_attribute(window, "title".into(), "Demo".into())
            .unwrap();
        dom.append_child(dom.root(), window).unwrap();

        let spec = WindowSpec::from_publication(&publication(&dom))
            .unwrap()
            .unwrap();

        assert_eq!(spec.dom_id(), window);
        assert_eq!(spec.title(), "Demo");
        assert_eq!(spec.inner_size(), LogicalSize::new(640.0, 480.0));
    }

    #[test]
    fn auto_and_percent_sizes_resolve_against_the_documented_default() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        dom.set_style_property(window, "width", "50%").unwrap();
        dom.append_child(dom.root(), window).unwrap();

        let spec = WindowSpec::from_publication(&publication(&dom))
            .unwrap()
            .unwrap();

        assert_eq!(spec.title(), "Burokku");
        assert_eq!(spec.inner_size(), LogicalSize::new(400.0, 600.0));
    }

    #[test]
    fn zero_initial_dimensions_are_rejected_before_native_creation() {
        let mut dom = Dom::new();
        let window = dom.create_element(Element::from_tag(ElementTag::Window));
        dom.set_style_property(window, "width", "0px").unwrap();
        dom.append_child(dom.root(), window).unwrap();

        let error = WindowSpec::from_publication(&publication(&dom)).unwrap_err();

        assert!(matches!(error, WindowHostError::InvalidInitialSize { .. }));
    }
}
