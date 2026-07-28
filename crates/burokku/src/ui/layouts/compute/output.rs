use render::{Clip, Rect as RenderRect, Transform};
use taffy::{geometry::Point, prelude::Display};

use crate::ui::{
    elements::{
        styles::{Isolation, Overflow as ElementOverflow, ZIndex},
        ElementKind,
    },
    layouts::{Layout, LayoutKind, ScrollOffset},
};

use super::{
    paint::{
        anchored_transform, box_style, multiply_transform, relative_transform,
        relative_transform_matrix,
    },
    scroll::{overflow_clip, padding_box, scroll_container, scroll_content_size, OffsetContext},
    tree::ElementLayoutTree,
};

impl ElementLayoutTree<'_> {
    pub(super) fn to_layout(
        &self,
        node: usize,
        parent_location: Point<f32>,
        ancestor_clips: &[Clip],
        viewport: RenderRect,
        parent_transform: Transform,
        flex_or_grid_item: bool,
    ) -> Layout {
        let data = &self.nodes[node];
        let location = Point {
            x: parent_location.x + data.layout.location.x,
            y: parent_location.y + data.layout.location.y,
        };
        let width = data.layout.size.width;
        let height = data.layout.size.height;
        let center = [location.x + width * 0.5, location.y + height * 0.5];
        let world_transform = multiply_transform(
            parent_transform,
            anchored_transform(
                Transform {
                    matrix: data.paint_style.transform.matrix(),
                },
                center,
            ),
        );
        let relative_transform = relative_transform(world_transform, center);
        let mut descendant_clips = ancestor_clips.to_vec();
        let mut own_clip = overflow_clip(data, location, width, height, viewport);
        if let Some(clip) = &mut own_clip {
            let clip_center = [
                clip.rect.x + clip.rect.width * 0.5,
                clip.rect.y + clip.rect.height * 0.5,
            ];
            clip.transform = relative_transform_matrix(world_transform, clip_center);
        }
        if let Some(clip) = own_clip {
            descendant_clips.push(clip);
        }
        let is_text_flow = matches!(data.kind, ElementKind::Text(_)) || data.inline_spans.is_some();
        let (kind, scroll) = if is_text_flow {
            let mut style = data.text_style.clone();
            style.opacity = data.paint_style.opacity;
            style.transform = Transform {
                matrix: data.paint_style.transform.matrix(),
            };
            (
                LayoutKind::Text {
                    text: data
                        .rendered_spans
                        .iter()
                        .map(|span| span.text.as_str())
                        .collect(),
                    spans: data.rendered_spans.clone(),
                    style,
                    has_transform: !data.paint_style.transform.is_none(),
                    line_count: data.text_line_count,
                    runs: data.text_runs.clone(),
                },
                None,
            )
        } else {
            match &data.kind {
                ElementKind::Comment(_)
                | ElementKind::Button
                | ElementKind::Div
                | ElementKind::Heading(_)
                | ElementKind::Image
                | ElementKind::Select
                | ElementKind::Span
                | ElementKind::Body
                | ElementKind::Other(_) => {
                    let scrolls_x = matches!(
                        data.paint_style.overflow_x,
                        ElementOverflow::Auto | ElementOverflow::Scroll
                    );
                    let scrolls_y = matches!(
                        data.paint_style.overflow_y,
                        ElementOverflow::Auto | ElementOverflow::Scroll
                    );
                    let requested = self
                        .scroll_offsets
                        .get(&data.element_id)
                        .copied()
                        .unwrap_or(ScrollOffset::ZERO);
                    let mut offset = ScrollOffset::new(
                        if scrolls_x { requested.x.max(0.0) } else { 0.0 },
                        if scrolls_y { requested.y.max(0.0) } else { 0.0 },
                    );
                    let child_parent = Point {
                        x: location.x - offset.x,
                        y: location.y - offset.y,
                    };
                    let children_are_flex_or_grid_items =
                        matches!(data.paint_style.display, Display::Flex | Display::Grid);
                    let mut children: Vec<_> = data
                        .children
                        .iter()
                        .map(|child| {
                            self.to_layout(
                                *child,
                                child_parent,
                                &descendant_clips,
                                viewport,
                                world_transform,
                                children_are_flex_or_grid_items,
                            )
                        })
                        .collect();
                    let scroll_viewport = padding_box(data, location, width, height);
                    let (content_width, content_height) =
                        scroll_content_size(&children, scroll_viewport, offset);
                    let max_offset = ScrollOffset::new(
                        if scrolls_x {
                            (content_width - scroll_viewport.width).max(0.0)
                        } else {
                            0.0
                        },
                        if scrolls_y {
                            (content_height - scroll_viewport.height).max(0.0)
                        } else {
                            0.0
                        },
                    );
                    let clamped =
                        ScrollOffset::new(offset.x.min(max_offset.x), offset.y.min(max_offset.y));
                    if clamped != offset {
                        offset = clamped;
                        let child_parent = Point {
                            x: location.x - offset.x,
                            y: location.y - offset.y,
                        };
                        children = data
                            .children
                            .iter()
                            .map(|child| {
                                self.to_layout(
                                    *child,
                                    child_parent,
                                    &descendant_clips,
                                    viewport,
                                    world_transform,
                                    children_are_flex_or_grid_items,
                                )
                            })
                            .collect();
                    }
                    let scroll = (scrolls_x || scrolls_y).then(|| {
                        scroll_container(
                            scroll_viewport,
                            own_clip.expect("scroll containers establish an overflow clip"),
                            content_width,
                            content_height,
                            OffsetContext::new(offset, max_offset),
                            data.paint_style.overflow_x == ElementOverflow::Scroll,
                            data.paint_style.overflow_y == ElementOverflow::Scroll,
                        )
                    });
                    (
                        LayoutKind::Box {
                            style: box_style(
                                &data.paint_style,
                                width,
                                height,
                                data.paint_style.opacity,
                                Transform {
                                    matrix: data.paint_style.transform.matrix(),
                                },
                            ),
                            has_transform: !data.paint_style.transform.is_none(),
                            z_index: match data.paint_style.z_index {
                                ZIndex::Auto => None,
                                ZIndex::Value(index) => Some(index),
                            },
                            isolated: data.paint_style.isolation == Isolation::Isolate,
                            position: data.paint_style.position,
                            flex_or_grid_item,
                            children,
                        },
                        scroll,
                    )
                }
                ElementKind::Text(_) => {
                    unreachable!("text nodes are handled as text flows")
                }
            }
        };

        Layout {
            element_id: data.element_id,
            x: location.x,
            y: location.y,
            width,
            height,
            transform: relative_transform,
            clips: ancestor_clips.to_vec(),
            scroll,
            kind,
        }
    }
}
