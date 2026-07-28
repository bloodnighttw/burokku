use render::{Clip, Rect as RenderRect, Transform};
use taffy::{geometry::Point, prelude::Display};

use crate::ui::{
    elements::{
        styles::{Overflow as ElementOverflow, Position, SizeValue},
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

#[derive(Clone, Copy)]
struct ContainingBlock {
    child_parent: Point<f32>,
    world_transform: Transform,
    viewport: bool,
}

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
        self.to_layout_with_fixed_containing_block(
            node,
            parent_location,
            ancestor_clips,
            viewport,
            parent_transform,
            flex_or_grid_item,
            ContainingBlock {
                child_parent: Point::ZERO,
                world_transform: Transform::IDENTITY,
                viewport: true,
            },
            ContainingBlock {
                child_parent: Point::ZERO,
                world_transform: Transform::IDENTITY,
                viewport: true,
            },
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "retained output needs normal and fixed containing-block state"
    )]
    fn to_layout_with_fixed_containing_block(
        &self,
        node: usize,
        parent_location: Point<f32>,
        ancestor_clips: &[Clip],
        viewport: RenderRect,
        parent_transform: Transform,
        flex_or_grid_item: bool,
        absolute_containing_block: ContainingBlock,
        fixed_containing_block: ContainingBlock,
    ) -> Layout {
        let data = &self.nodes[node];
        let mut relative_location = data.layout.location;
        if data.positioning_containing_block.is_some() {
            if data.paint_style.left == SizeValue::Auto && data.paint_style.right == SizeValue::Auto
            {
                relative_location.x = data.static_offset.x;
            }
            if data.paint_style.top == SizeValue::Auto && data.paint_style.bottom == SizeValue::Auto
            {
                relative_location.y = data.static_offset.y;
            }
        }
        let location = Point {
            x: parent_location.x + relative_location.x,
            y: parent_location.y + relative_location.y,
        };
        let width = data.layout.size.width;
        let height = data.layout.size.height;
        let center = [location.x + width * 0.5, location.y + height * 0.5];
        let world_transform = multiply_transform(
            parent_transform,
            anchored_transform(data.paint_style.transform.into(), center),
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
            style.transform = data.paint_style.transform.into();
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
                    let descendant_fixed_containing_block = if !data.paint_style.transform.is_none()
                    {
                        ContainingBlock {
                            child_parent,
                            world_transform,
                            viewport: false,
                        }
                    } else {
                        fixed_containing_block
                    };
                    let descendant_absolute_containing_block =
                        if data.paint_style.position.is_positioned()
                            || !data.paint_style.transform.is_none()
                        {
                            ContainingBlock {
                                child_parent,
                                world_transform,
                                viewport: false,
                            }
                        } else {
                            absolute_containing_block
                        };
                    let children_are_flex_or_grid_items =
                        matches!(data.paint_style.display, Display::Flex | Display::Grid);
                    let mut children: Vec<_> = data
                        .children
                        .iter()
                        .map(|child| {
                            let child_data = &self.nodes[*child];
                            let (parent_location, clips, parent_transform) =
                                if child_data.paint_style.position == Position::Fixed
                                    && child_data.positioning_containing_block.is_some()
                                {
                                    (
                                        descendant_fixed_containing_block.child_parent,
                                        if descendant_fixed_containing_block.viewport {
                                            &[][..]
                                        } else {
                                            descendant_clips.as_slice()
                                        },
                                        descendant_fixed_containing_block.world_transform,
                                    )
                                } else if child_data.paint_style.position == Position::Absolute
                                    && child_data.positioning_containing_block.is_some()
                                {
                                    (
                                        descendant_absolute_containing_block.child_parent,
                                        descendant_clips.as_slice(),
                                        descendant_absolute_containing_block.world_transform,
                                    )
                                } else {
                                    (child_parent, descendant_clips.as_slice(), world_transform)
                                };
                            self.to_layout_with_fixed_containing_block(
                                *child,
                                parent_location,
                                clips,
                                viewport,
                                parent_transform,
                                children_are_flex_or_grid_items,
                                descendant_absolute_containing_block,
                                descendant_fixed_containing_block,
                            )
                        })
                        .collect();
                    let scroll_viewport = padding_box(data, location, width, height);
                    let (content_width, content_height) = scroll_content_size(
                        children
                            .iter()
                            .filter(|child| !child.is_fixed_to_viewport()),
                        scroll_viewport,
                        offset,
                    );
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
                        let descendant_fixed_containing_block =
                            if !data.paint_style.transform.is_none() {
                                ContainingBlock {
                                    child_parent,
                                    world_transform,
                                    viewport: false,
                                }
                            } else {
                                fixed_containing_block
                            };
                        let descendant_absolute_containing_block =
                            if data.paint_style.position.is_positioned()
                                || !data.paint_style.transform.is_none()
                            {
                                ContainingBlock {
                                    child_parent,
                                    world_transform,
                                    viewport: false,
                                }
                            } else {
                                absolute_containing_block
                            };
                        children = data
                            .children
                            .iter()
                            .map(|child| {
                                let child_data = &self.nodes[*child];
                                let (parent_location, clips, parent_transform) =
                                    if child_data.paint_style.position == Position::Fixed
                                        && child_data.positioning_containing_block.is_some()
                                    {
                                        (
                                            descendant_fixed_containing_block.child_parent,
                                            if descendant_fixed_containing_block.viewport {
                                                &[][..]
                                            } else {
                                                descendant_clips.as_slice()
                                            },
                                            descendant_fixed_containing_block.world_transform,
                                        )
                                    } else if child_data.paint_style.position == Position::Absolute
                                        && child_data.positioning_containing_block.is_some()
                                    {
                                        (
                                            descendant_absolute_containing_block.child_parent,
                                            descendant_clips.as_slice(),
                                            descendant_absolute_containing_block.world_transform,
                                        )
                                    } else {
                                        (child_parent, descendant_clips.as_slice(), world_transform)
                                    };
                                self.to_layout_with_fixed_containing_block(
                                    *child,
                                    parent_location,
                                    clips,
                                    viewport,
                                    parent_transform,
                                    children_are_flex_or_grid_items,
                                    descendant_absolute_containing_block,
                                    descendant_fixed_containing_block,
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
                                data.paint_style.transform.into(),
                            ),
                            has_transform: !data.paint_style.transform.is_none(),
                            z_index: data.paint_style.z_index.into(),
                            isolated: data.paint_style.isolation.into(),
                            position: data.paint_style.position,
                            fixed_containing_block: if data.paint_style.position == Position::Fixed
                            {
                                data.positioning_containing_block
                                    .filter(|owner| *owner != self.viewport_root)
                                    .map(|owner| self.nodes[owner].element_id)
                            } else {
                                None
                            },
                            fixed_to_viewport: data.paint_style.position == Position::Fixed
                                && data.positioning_containing_block == Some(self.viewport_root),
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
