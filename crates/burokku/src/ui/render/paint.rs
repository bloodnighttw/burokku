use render::{Canvas, PaintLayer, Rect, Transform};

use super::{
    node::paint_phased_layout,
    scale::{scaled_clip, scaled_transform},
    scrollbars::paint_context_scrollbars,
    transform::{establishes_effect_group, layout_effect, layout_world_transform, localize_clip},
};
use crate::ui::layouts::{
    descendant_contexts, zero_level_entries, Layout, Stacking, ZeroLevelEntry,
};

pub(super) fn paint_layout(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    canvas: &mut Canvas,
) {
    if establishes_effect_group(layout) {
        paint_atomic_context(
            layout,
            viewport,
            scale_factor,
            0,
            PaintLayer::ContextBackground,
            Transform::IDENTITY,
            canvas,
        );
    } else {
        paint_stacking_context(
            layout,
            viewport,
            scale_factor,
            0,
            false,
            Transform::IDENTITY,
            canvas,
        );
    }
}

fn paint_stacking_context(
    root: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    strip_root_effect: bool,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    paint_phased_layout(
        root,
        PaintLayer::ContextBackground,
        viewport,
        scale_factor,
        clip_skip,
        strip_root_effect,
        context_world,
        canvas,
    );

    let contexts = descendant_contexts(root);
    for context in contexts
        .iter()
        .copied()
        .filter(|layout| layout.stacking_index() < 0)
    {
        paint_atomic_context(
            context,
            viewport,
            scale_factor,
            clip_skip,
            PaintLayer::Negative,
            context_world,
            canvas,
        );
    }

    paint_ordinary_descendants(
        root,
        viewport,
        scale_factor,
        clip_skip,
        context_world,
        canvas,
    );
    paint_context_scrollbars(
        root,
        viewport,
        scale_factor,
        clip_skip,
        context_world,
        canvas,
    );

    for entry in zero_level_entries(root) {
        match entry {
            ZeroLevelEntry::Context(context) => paint_atomic_context(
                context,
                viewport,
                scale_factor,
                clip_skip,
                PaintLayer::Positioned,
                context_world,
                canvas,
            ),
            ZeroLevelEntry::PositionedAuto(layout) => paint_positioned_auto(
                layout,
                viewport,
                scale_factor,
                clip_skip,
                context_world,
                canvas,
            ),
        }
    }

    for context in contexts
        .iter()
        .copied()
        .filter(|layout| layout.stacking_index() > 0)
    {
        paint_atomic_context(
            context,
            viewport,
            scale_factor,
            clip_skip,
            PaintLayer::Positioned,
            context_world,
            canvas,
        );
    }
}

fn paint_atomic_context(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    layer: PaintLayer,
    parent_context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut group = Canvas::new();
    let has_effect = establishes_effect_group(layout);
    let context_world = if has_effect {
        layout_world_transform(layout)
    } else {
        parent_context_world
    };
    paint_stacking_context(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        has_effect,
        context_world,
        &mut group,
    );
    if group.commands().is_empty() {
        return;
    }
    let (opacity, transform) = layout_effect(layout);
    canvas.draw_group_in_layer(
        layer,
        group,
        [
            (layout.x + layout.width * 0.5) * scale_factor,
            (layout.y + layout.height * 0.5) * scale_factor,
        ],
        scaled_transform(transform, scale_factor),
        if has_effect { opacity } else { 1.0 },
        layout
            .clips
            .iter()
            .skip(clip_skip)
            .copied()
            .map(|clip| localize_clip(clip, parent_context_world))
            .map(|clip| scaled_clip(clip, scale_factor)),
    );
}

fn paint_positioned_auto(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    parent_context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut group = Canvas::new();
    paint_phased_layout(
        layout,
        PaintLayer::ContextBackground,
        viewport,
        scale_factor,
        layout.clips.len(),
        false,
        parent_context_world,
        &mut group,
    );
    paint_ordinary_descendants(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        parent_context_world,
        &mut group,
    );
    paint_context_scrollbars(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        parent_context_world,
        &mut group,
    );
    if group.commands().is_empty() {
        return;
    }
    canvas.draw_group_in_layer(
        PaintLayer::Positioned,
        group,
        [
            (layout.x + layout.width * 0.5) * scale_factor,
            (layout.y + layout.height * 0.5) * scale_factor,
        ],
        Transform::IDENTITY,
        1.0,
        layout
            .clips
            .iter()
            .skip(clip_skip)
            .copied()
            .map(|clip| localize_clip(clip, parent_context_world))
            .map(|clip| scaled_clip(clip, scale_factor)),
    );
}

fn paint_ordinary_descendants(
    root: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut pending = vec![root.children().iter()];
    while let Some(mut children) = pending.pop() {
        let Some(layout) = children.next() else {
            continue;
        };
        pending.push(children);
        if layout.establishes_stacking_context() || layout.is_positioned_auto() {
            continue;
        }
        if layout.is_flex_or_grid_item_auto() {
            paint_atomic_flex_or_grid_item(
                layout,
                viewport,
                scale_factor,
                clip_skip,
                context_world,
                canvas,
            );
            continue;
        }
        paint_phased_layout(
            layout,
            PaintLayer::Block,
            viewport,
            scale_factor,
            clip_skip,
            false,
            context_world,
            canvas,
        );
        pending.push(layout.children().iter());
    }
}

fn paint_atomic_flex_or_grid_item(
    layout: &Layout,
    viewport: Rect,
    scale_factor: f32,
    clip_skip: usize,
    context_world: Transform,
    canvas: &mut Canvas,
) {
    let mut group = Canvas::new();
    paint_phased_layout(
        layout,
        PaintLayer::ContextBackground,
        viewport,
        scale_factor,
        layout.clips.len(),
        false,
        context_world,
        &mut group,
    );
    paint_ordinary_descendants(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        context_world,
        &mut group,
    );
    paint_context_scrollbars(
        layout,
        viewport,
        scale_factor,
        layout.clips.len(),
        context_world,
        &mut group,
    );
    if group.commands().is_empty() {
        return;
    }
    canvas.draw_group_in_layer(
        PaintLayer::Content,
        group,
        [
            (layout.x + layout.width * 0.5) * scale_factor,
            (layout.y + layout.height * 0.5) * scale_factor,
        ],
        Transform::IDENTITY,
        1.0,
        layout
            .clips
            .iter()
            .skip(clip_skip)
            .copied()
            .map(|clip| localize_clip(clip, context_world))
            .map(|clip| scaled_clip(clip, scale_factor)),
    );
}
