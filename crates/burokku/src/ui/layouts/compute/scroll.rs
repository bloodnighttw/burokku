use render::{Clip, CornerRadius, Rect as RenderRect, Transform};
use taffy::geometry::Point;

use crate::ui::elements::styles::{Overflow as ElementOverflow, Style as ElementStyle};
use crate::ui::layouts::{ScrollContainer, Scrollbar, ScrollbarAxis};

use super::paint::box_style;
use super::{Layout, ScrollOffset};
use crate::ui::layouts::layout_node::TaffyNode;

#[derive(Clone, Copy)]
pub(super) struct OffsetContext {
    offset: ScrollOffset,
    max_offset: ScrollOffset,
}

impl OffsetContext {
    pub(super) fn new(offset: ScrollOffset, max_offset: ScrollOffset) -> Self {
        Self { offset, max_offset }
    }
}

pub(super) fn padding_box(
    data: &TaffyNode,
    location: Point<f32>,
    width: f32,
    height: f32,
) -> RenderRect {
    let border = data.layout.border;
    RenderRect::new(
        location.x + border.left,
        location.y + border.top,
        (width - border.left - border.right).max(0.0),
        (height - border.top - border.bottom).max(0.0),
    )
}

pub(super) fn scroll_content_size<'a>(
    children: impl IntoIterator<Item = &'a Layout>,
    viewport: RenderRect,
    offset: ScrollOffset,
) -> (f32, f32) {
    children.into_iter().fold(
        (viewport.width, viewport.height),
        |(width, height), child| {
            (
                width.max(child.x + offset.x + child.width - viewport.x),
                height.max(child.y + offset.y + child.height - viewport.y),
            )
        },
    )
}

pub(super) fn scroll_container(
    viewport: RenderRect,
    clip: Clip,
    content_width: f32,
    content_height: f32,
    offsets: OffsetContext,
    always_show_horizontal: bool,
    always_show_vertical: bool,
) -> ScrollContainer {
    const INSET: f32 = 2.0;
    const THICKNESS: f32 = 8.0;
    const CROSS_AXIS_SPACE: f32 = 12.0;
    const MIN_THUMB: f32 = 24.0;

    let OffsetContext { offset, max_offset } = offsets;
    let has_horizontal = always_show_horizontal || max_offset.x > 0.0;
    let has_vertical = always_show_vertical || max_offset.y > 0.0;
    let horizontal = has_horizontal.then(|| {
        let track = RenderRect::new(
            viewport.x + INSET,
            viewport.y + viewport.height - THICKNESS - INSET,
            (viewport.width - INSET * 2.0 - if has_vertical { CROSS_AXIS_SPACE } else { 0.0 })
                .max(0.0),
            THICKNESS,
        );
        Scrollbar {
            axis: ScrollbarAxis::Horizontal,
            track,
            thumb: scrollbar_thumb(
                track,
                ScrollbarAxis::Horizontal,
                viewport.width,
                content_width,
                offsets,
                MIN_THUMB,
            ),
        }
    });
    let vertical = has_vertical.then(|| {
        let track = RenderRect::new(
            viewport.x + viewport.width - THICKNESS - INSET,
            viewport.y + INSET,
            THICKNESS,
            (viewport.height
                - INSET * 2.0
                - if has_horizontal {
                    CROSS_AXIS_SPACE
                } else {
                    0.0
                })
            .max(0.0),
        );
        Scrollbar {
            axis: ScrollbarAxis::Vertical,
            track,
            thumb: scrollbar_thumb(
                track,
                ScrollbarAxis::Vertical,
                viewport.height,
                content_height,
                offsets,
                MIN_THUMB,
            ),
        }
    });

    ScrollContainer {
        viewport,
        clip,
        content_width,
        content_height,
        offset,
        max_offset,
        horizontal,
        vertical,
    }
}

fn scrollbar_thumb(
    track: RenderRect,
    axis: ScrollbarAxis,
    viewport_size: f32,
    content_size: f32,
    offsets: OffsetContext,
    min_thumb: f32,
) -> RenderRect {
    let (offset, max_offset) = match axis {
        ScrollbarAxis::Horizontal => (offsets.offset.x, offsets.max_offset.x),
        ScrollbarAxis::Vertical => (offsets.offset.y, offsets.max_offset.y),
    };
    let track_size = match axis {
        ScrollbarAxis::Horizontal => track.width,
        ScrollbarAxis::Vertical => track.height,
    };
    let thumb_size = (track_size * viewport_size / content_size.max(viewport_size))
        .clamp(min_thumb.min(track_size), track_size);
    let travel = (track_size - thumb_size).max(0.0);
    let position = if max_offset > 0.0 {
        travel * offset / max_offset
    } else {
        0.0
    };
    match axis {
        ScrollbarAxis::Horizontal => {
            RenderRect::new(track.x + position, track.y, thumb_size, track.height)
        }
        ScrollbarAxis::Vertical => {
            RenderRect::new(track.x, track.y + position, track.width, thumb_size)
        }
    }
}

pub(super) fn overflow_clip(
    data: &TaffyNode,
    style: &ElementStyle,
    location: Point<f32>,
    width: f32,
    height: f32,
    viewport: RenderRect,
) -> Option<Clip> {
    let clips_x = style.overflow_x != ElementOverflow::Visible;
    let clips_y = style.overflow_y != ElementOverflow::Visible;
    if !clips_x && !clips_y {
        return None;
    }

    let border = data.layout.border;
    let padding_box = padding_box(data, location, width, height);
    let rect = RenderRect::new(
        if clips_x { padding_box.x } else { viewport.x },
        if clips_y { padding_box.y } else { viewport.y },
        if clips_x {
            padding_box.width
        } else {
            viewport.width
        },
        if clips_y {
            padding_box.height
        } else {
            viewport.height
        },
    );
    let corner_radius = if clips_x && clips_y {
        let outer = box_style(style, width, height, 1.0, Transform::IDENTITY).corner_radius;
        CornerRadius::new(
            (outer.top_left - border.left.max(border.top)).max(0.0),
            (outer.top_right - border.right.max(border.top)).max(0.0),
            (outer.bottom_right - border.right.max(border.bottom)).max(0.0),
            (outer.bottom_left - border.left.max(border.bottom)).max(0.0),
        )
    } else {
        CornerRadius::ZERO
    };
    Some(Clip::new(rect, corner_radius))
}
