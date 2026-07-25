use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::Arc,
};

use render::{wgpu, RenderError, RenderTimings, Renderer, SurfaceSize, TextSystem};
use winit::{dpi::PhysicalSize, window::Window};

use crate::ui::{
    self,
    layouts::{ScrollOffset, ScrollbarAxis},
    Document, UiFrame, UiStore,
};

#[derive(Clone, Copy, Debug)]
struct ScrollDrag {
    element_id: u64,
    axis: ScrollbarAxis,
    pointer_start: f32,
    offset_start: f32,
}

/// The WebGPU state used by the application window.
#[allow(clippy::upper_case_acronyms)]
pub struct GPU {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    frame: UiFrame,
    text_system: TextSystem,
    store: UiStore,
    ui_version: u64,
    scroll_offsets: HashMap<u64, ScrollOffset>,
    scroll_drag: Option<ScrollDrag>,
    canvas_dirty: bool,
    // The instance must stay alive for as long as the surface is in use.
    _instance: wgpu::Instance,
}

impl GPU {
    pub async fn new(window: Arc<Window>, store: UiStore) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let size = window.inner_size();
        let renderer = Renderer::new(
            &instance,
            &surface,
            SurfaceSize::new(size.width, size.height),
        )
        .await?;

        let mut text_system = TextSystem::new();
        let (ui_version, snapshot) = store.snapshot_with_version();
        let scroll_offsets = HashMap::new();
        let frame = build_frame(
            &snapshot,
            size,
            window.scale_factor(),
            &mut text_system,
            &scroll_offsets,
        );

        Ok(Self {
            surface,
            renderer,
            frame,
            text_system,
            store,
            ui_version,
            scroll_offsets,
            scroll_drag: None,
            canvas_dirty: false,
            _instance: instance,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f64) {
        self.renderer
            .resize(&self.surface, SurfaceSize::new(size.width, size.height));
        let (ui_version, snapshot) = self.store.snapshot_with_version();
        self.frame = build_frame(
            &snapshot,
            size,
            scale_factor,
            &mut self.text_system,
            &self.scroll_offsets,
        );
        self.ui_version = ui_version;
        self.canvas_dirty = false;
        prune_scroll_offsets(&mut self.scroll_offsets, &self.frame.layout);
    }

    pub fn sync_ui(&mut self, window: &Window) -> bool {
        let Some((version, snapshot)) = self.store.snapshot_if_changed(self.ui_version) else {
            return false;
        };
        self.frame = build_frame(
            &snapshot,
            window.inner_size(),
            window.scale_factor(),
            &mut self.text_system,
            &self.scroll_offsets,
        );
        self.ui_version = version;
        self.canvas_dirty = false;
        prune_scroll_offsets(&mut self.scroll_offsets, &self.frame.layout);
        true
    }

    pub fn render(&mut self, window: &Window) -> Result<RenderTimings, RenderError> {
        if self.canvas_dirty {
            ui::repaint_frame(&mut self.frame, window.scale_factor() as f32);
            self.canvas_dirty = false;
        }
        self.renderer.render_timed_with_pre_present(
            &self.surface,
            &self.frame.canvas,
            &mut self.text_system,
            || window.pre_present_notify(),
        )
    }

    pub fn scroll_wheel(
        &mut self,
        window: &Window,
        cursor_x: f64,
        cursor_y: f64,
        delta_x: f64,
        delta_y: f64,
        precise: bool,
    ) -> bool {
        let scale_factor = window.scale_factor();
        let x = (cursor_x / scale_factor) as f32;
        let y = (cursor_y / scale_factor) as f32;
        let multiplier = if precise { 1.0 } else { 40.0 };
        let delta = ScrollOffset::new(
            (-delta_x * multiplier) as f32,
            (-delta_y * multiplier) as f32,
        );

        let target = wheel_target(&self.frame.layout, x, y, delta);
        let Some((element_id, offset)) = target else {
            return false;
        };
        self.scroll_offsets.insert(element_id, offset);
        self.apply_scroll_offset(window, element_id, offset);
        true
    }

    pub fn begin_scroll_drag(&mut self, window: &Window, cursor_x: f64, cursor_y: f64) -> bool {
        let scale_factor = window.scale_factor();
        let x = (cursor_x / scale_factor) as f32;
        let y = (cursor_y / scale_factor) as f32;
        let target = self.frame.layout.iter_rev().find_map(|layout| {
            let scroll = layout.scroll?;
            if !layout.clips.iter().all(|clip| clip.contains(x, y)) {
                return None;
            }
            let scrollbar = scroll.scrollbar_at(x, y)?;
            Some((layout.element_id, scroll, scrollbar))
        });
        let Some((element_id, scroll, scrollbar)) = target else {
            return false;
        };

        if scrollbar.thumb.contains(x, y) {
            self.scroll_drag = Some(ScrollDrag {
                element_id,
                axis: scrollbar.axis,
                pointer_start: axis_position(scrollbar.axis, x, y),
                offset_start: axis_offset(scrollbar.axis, scroll.offset),
            });
            return true;
        }

        let pointer = axis_position(scrollbar.axis, x, y);
        let thumb_start = axis_rect_start(scrollbar.axis, scrollbar.thumb);
        let page = axis_viewport_size(scrollbar.axis, scroll.viewport) * 0.9;
        let current = axis_offset(scrollbar.axis, scroll.offset);
        let next = if pointer < thumb_start {
            current - page
        } else {
            current + page
        };
        let offset = with_axis_offset(
            scrollbar.axis,
            scroll.offset,
            next.clamp(0.0, axis_offset(scrollbar.axis, scroll.max_offset)),
        );
        self.scroll_offsets.insert(element_id, offset);
        self.apply_scroll_offset(window, element_id, offset);
        true
    }

    pub fn update_scroll_drag(&mut self, window: &Window, cursor_x: f64, cursor_y: f64) -> bool {
        let Some(drag) = self.scroll_drag else {
            return false;
        };
        let Some(layout) = self
            .frame
            .layout
            .iter()
            .find(|layout| layout.element_id == drag.element_id)
        else {
            self.scroll_drag = None;
            return false;
        };
        let Some(scroll) = layout.scroll else {
            self.scroll_drag = None;
            return false;
        };
        let scrollbar = match drag.axis {
            ScrollbarAxis::Horizontal => scroll.horizontal,
            ScrollbarAxis::Vertical => scroll.vertical,
        };
        let Some(scrollbar) = scrollbar else {
            self.scroll_drag = None;
            return false;
        };

        let scale_factor = window.scale_factor();
        let x = (cursor_x / scale_factor) as f32;
        let y = (cursor_y / scale_factor) as f32;
        let pointer = axis_position(drag.axis, x, y);
        let track_size = axis_rect_size(drag.axis, scrollbar.track);
        let thumb_size = axis_rect_size(drag.axis, scrollbar.thumb);
        let travel = track_size - thumb_size;
        let max_offset = axis_offset(drag.axis, scroll.max_offset);
        if travel <= 0.0 || max_offset <= 0.0 {
            return false;
        }
        let next = dragged_offset(
            drag.offset_start,
            pointer - drag.pointer_start,
            travel,
            max_offset,
        );
        let offset = with_axis_offset(drag.axis, scroll.offset, next);
        if offset == scroll.offset {
            return false;
        }
        self.scroll_offsets.insert(drag.element_id, offset);
        self.apply_scroll_offset(window, drag.element_id, offset);
        true
    }

    pub fn end_scroll_drag(&mut self) {
        self.scroll_drag = None;
    }

    /// The current logical layout, retained for hit testing and input routing.
    #[allow(dead_code)]
    pub fn layout(&self) -> &ui::layouts::Layout {
        &self.frame.layout
    }

    fn rebuild(&mut self, window: &Window) {
        let (version, snapshot) = self.store.snapshot_with_version();
        self.frame = build_frame(
            &snapshot,
            window.inner_size(),
            window.scale_factor(),
            &mut self.text_system,
            &self.scroll_offsets,
        );
        self.ui_version = version;
        self.canvas_dirty = false;
        prune_scroll_offsets(&mut self.scroll_offsets, &self.frame.layout);
    }

    fn apply_scroll_offset(&mut self, window: &Window, element_id: u64, offset: ScrollOffset) {
        if self.frame.layout.apply_scroll_offset(element_id, offset) {
            self.canvas_dirty = true;
        } else {
            self.rebuild(window);
        }
    }
}

fn prune_scroll_offsets(
    scroll_offsets: &mut HashMap<u64, ScrollOffset>,
    root: &ui::layouts::Layout,
) {
    let active_scroll_ids: HashSet<_> = root
        .iter()
        .filter(|layout| layout.scroll.is_some())
        .map(|layout| layout.element_id)
        .collect();
    scroll_offsets.retain(|element_id, _| active_scroll_ids.contains(element_id));
}

fn axis_position(axis: ScrollbarAxis, x: f32, y: f32) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => x,
        ScrollbarAxis::Vertical => y,
    }
}

fn axis_offset(axis: ScrollbarAxis, offset: ScrollOffset) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => offset.x,
        ScrollbarAxis::Vertical => offset.y,
    }
}

fn with_axis_offset(axis: ScrollbarAxis, offset: ScrollOffset, value: f32) -> ScrollOffset {
    match axis {
        ScrollbarAxis::Horizontal => ScrollOffset::new(value, offset.y),
        ScrollbarAxis::Vertical => ScrollOffset::new(offset.x, value),
    }
}

fn axis_rect_start(axis: ScrollbarAxis, rect: render::Rect) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => rect.x,
        ScrollbarAxis::Vertical => rect.y,
    }
}

fn axis_rect_size(axis: ScrollbarAxis, rect: render::Rect) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => rect.width,
        ScrollbarAxis::Vertical => rect.height,
    }
}

fn axis_viewport_size(axis: ScrollbarAxis, viewport: render::Rect) -> f32 {
    axis_rect_size(axis, viewport)
}

fn dragged_offset(offset_start: f32, pointer_delta: f32, travel: f32, max_offset: f32) -> f32 {
    if travel <= 0.0 || max_offset <= 0.0 {
        return 0.0;
    }
    (offset_start + pointer_delta * max_offset / travel).clamp(0.0, max_offset)
}

fn wheel_target(
    root: &ui::layouts::Layout,
    x: f32,
    y: f32,
    delta: ScrollOffset,
) -> Option<(u64, ScrollOffset)> {
    root.iter_rev().find_map(|layout| {
        let scroll = layout.scroll?;
        if !scroll.viewport.contains(x, y) || !layout.clips.iter().all(|clip| clip.contains(x, y)) {
            return None;
        }
        let next = ScrollOffset::new(
            (scroll.offset.x + delta.x).clamp(0.0, scroll.max_offset.x),
            (scroll.offset.y + delta.y).clamp(0.0, scroll.max_offset.y),
        );
        (next != scroll.offset).then_some((layout.element_id, next))
    })
}

fn build_frame(
    document: &Document,
    size: PhysicalSize<u32>,
    scale_factor: f64,
    text_system: &mut TextSystem,
    scroll_offsets: &HashMap<u64, ScrollOffset>,
) -> UiFrame {
    let scale_factor = scale_factor as f32;
    ui::build_frame_with_scroll(
        document,
        size.width as f32 / scale_factor,
        size.height as f32 / scale_factor,
        scale_factor,
        text_system,
        scroll_offsets,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{elements::ElementKind, layouts::compute_layout_with_scroll, Document};

    #[test]
    fn thumb_drag_maps_track_travel_to_the_full_scroll_range() {
        assert_eq!(dragged_offset(0.0, 50.0, 100.0, 300.0), 150.0);
        assert_eq!(dragged_offset(100.0, 100.0, 100.0, 300.0), 300.0);
        assert_eq!(dragged_offset(100.0, -100.0, 100.0, 300.0), 0.0);
    }

    #[test]
    fn wheel_targets_the_scrollable_box_under_the_pointer() {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let content = document.create_node(ElementKind::Div);
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("60px"))
            .unwrap();
        document
            .set_style(container, "overflow", Some("auto"))
            .unwrap();
        document.set_style(content, "width", Some("100px")).unwrap();
        document
            .set_style(content, "height", Some("200px"))
            .unwrap();
        document.insert(0, container, None).unwrap();
        document.insert(container, content, None).unwrap();
        let layout = compute_layout_with_scroll(
            &document,
            300.0,
            200.0,
            &mut TextSystem::new(),
            &HashMap::new(),
        );

        assert_eq!(
            wheel_target(&layout, 20.0, 20.0, ScrollOffset::new(0.0, 40.0)),
            Some((container, ScrollOffset::new(0.0, 40.0)))
        );
        assert_eq!(
            wheel_target(&layout, 200.0, 150.0, ScrollOffset::new(0.0, 40.0)),
            None
        );
    }

    #[test]
    fn full_rebuild_prunes_offsets_for_inactive_elements() {
        let mut document = Document::new();
        let active = document.create_node(ElementKind::Div);
        document.set_style(active, "width", Some("100px")).unwrap();
        document.set_style(active, "height", Some("60px")).unwrap();
        document
            .set_style(active, "overflow", Some("auto"))
            .unwrap();
        document.insert(0, active, None).unwrap();

        let layout = compute_layout_with_scroll(
            &document,
            300.0,
            200.0,
            &mut TextSystem::new(),
            &HashMap::new(),
        );
        let stale = active + 1;
        let mut offsets = HashMap::from([
            (active, ScrollOffset::new(0.0, 20.0)),
            (stale, ScrollOffset::new(0.0, 40.0)),
        ]);

        prune_scroll_offsets(&mut offsets, &layout);

        assert_eq!(
            offsets,
            HashMap::from([(active, ScrollOffset::new(0.0, 20.0))])
        );
    }
}
