//! Native ownership for the single committed `<window>` element.

use std::sync::Arc;

use thiserror::Error;
use winit::{ActiveEventLoop, EventLoop, LogicalSize, Window, WindowAttributes, WindowId};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowChange {
    Unchanged,
    Created,
    Updated,
    Replaced,
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
}

#[derive(Debug, Default)]
pub(crate) struct WindowManager {
    current: Option<NativeWindow>,
}

impl WindowManager {
    pub(crate) fn create_initial(
        event_loop: &mut EventLoop,
        publication: &PublishedDom,
    ) -> Result<Self, WindowHostError> {
        let spec =
            WindowSpec::from_publication(publication)?.ok_or(WindowHostError::MissingWindow)?;
        let window = Arc::new(event_loop.create_window(spec.attributes())?);
        Ok(Self {
            current: Some(NativeWindow { spec, window }),
        })
    }

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
                if current.spec.title != spec.title {
                    current.window.set_title(&spec.title);
                }
                if current.spec.inner_size != spec.inner_size {
                    current.window.set_inner_size(spec.inner_size)?;
                }
                current.spec = spec;
                current.window.request_redraw();
                Ok(WindowChange::Updated)
            }
            (Some(_), Some(spec)) => {
                // Create first so a platform failure leaves the previous native
                // window and its renderer intact.
                let replacement = Arc::new(event_loop.create_window(spec.attributes())?);
                let previous = self.current.replace(NativeWindow {
                    spec,
                    window: replacement,
                });
                if let Some(previous) = previous {
                    previous.window.close();
                }
                Ok(WindowChange::Replaced)
            }
        }
    }

    pub(crate) fn close(&mut self) {
        if let Some(current) = self.current.take() {
            current.window.close();
        }
    }
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

    #[error("the committed app has no Window child")]
    MissingWindow,

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
    use std::sync::Arc;

    use crate::ui::elements::{Dom, DomPublisher, ElementTag};

    use super::*;

    fn publication(dom: &Dom) -> Arc<PublishedDom> {
        let (_publisher, reader) = DomPublisher::new(dom, |_| {});
        reader.load()
    }

    #[test]
    fn absent_window_has_no_native_spec() {
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
