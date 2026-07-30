use render::{Clip, Transform};

use crate::ui::layouts::{Layout, LayoutKind};

pub(super) fn establishes_effect_group(layout: &Layout) -> bool {
    let (opacity, transform) = layout_effect(layout);
    opacity < 1.0 || transform != Transform::IDENTITY
}

pub(super) fn layout_effect(layout: &Layout) -> (f32, Transform) {
    match &layout.kind {
        LayoutKind::Box { style, .. } => (style.opacity, style.transform),
        LayoutKind::Text { style, .. } => (style.opacity, style.transform),
    }
}

pub(super) fn layout_world_transform(layout: &Layout) -> Transform {
    let center = [
        layout.x + layout.width * 0.5,
        layout.y + layout.height * 0.5,
    ];
    anchored_transform(layout.transform, center)
}

pub(super) fn localize_clip(mut clip: Clip, context_world: Transform) -> Clip {
    if context_world == Transform::IDENTITY {
        return clip;
    }
    let center = [
        clip.rect.x + clip.rect.width * 0.5,
        clip.rect.y + clip.rect.height * 0.5,
    ];
    let clip_world = anchored_transform(
        Transform {
            matrix: clip.transform,
        },
        center,
    );
    let Some(context_inverse) = inverse_transform(context_world) else {
        return clip;
    };
    let localized = multiply_transform(context_inverse, clip_world);
    clip.transform = relative_transform(localized, center).matrix;
    clip
}

fn anchored_transform(transform: Transform, center: [f32; 2]) -> Transform {
    let [a, b, c, d, tx, ty] = transform.matrix;
    Transform {
        matrix: [
            a,
            b,
            c,
            d,
            center[0] + tx - a * center[0] - c * center[1],
            center[1] + ty - b * center[0] - d * center[1],
        ],
    }
}

fn relative_transform(transform: Transform, center: [f32; 2]) -> Transform {
    let [a, b, c, d, tx, ty] = transform.matrix;
    Transform {
        matrix: [
            a,
            b,
            c,
            d,
            tx - center[0] + a * center[0] + c * center[1],
            ty - center[1] + b * center[0] + d * center[1],
        ],
    }
}

fn multiply_transform(left: Transform, right: Transform) -> Transform {
    let [la, lb, lc, ld, ltx, lty] = left.matrix;
    let [ra, rb, rc, rd, rtx, rty] = right.matrix;
    Transform {
        matrix: [
            la * ra + lc * rb,
            lb * ra + ld * rb,
            la * rc + lc * rd,
            lb * rc + ld * rd,
            la * rtx + lc * rty + ltx,
            lb * rtx + ld * rty + lty,
        ],
    }
}

fn inverse_transform(transform: Transform) -> Option<Transform> {
    let [a, b, c, d, tx, ty] = transform.matrix;
    let determinant = a * d - b * c;
    if determinant.abs() <= f32::EPSILON {
        return None;
    }
    let inverse = [
        d / determinant,
        -b / determinant,
        -c / determinant,
        a / determinant,
    ];
    Some(Transform {
        matrix: [
            inverse[0],
            inverse[1],
            inverse[2],
            inverse[3],
            -inverse[0] * tx - inverse[2] * ty,
            -inverse[1] * tx - inverse[3] * ty,
        ],
    })
}
