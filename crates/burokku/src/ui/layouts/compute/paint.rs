use render::{
    BackgroundImage, Border, BoxShadow, BoxStyle, Color, CornerRadius, GradientStop, Outline,
    Transform,
};

use crate::ui::elements::styles::{
    Color as ElementColor, LengthPercentageValue, Style as ElementStyle,
};

pub(super) fn box_style(
    style: &ElementStyle,
    width: f32,
    height: f32,
    opacity: f32,
    transform: Transform,
) -> BoxStyle {
    let border_width = [
        style.border_top_width.px(),
        style.border_right_width.px(),
        style.border_bottom_width.px(),
        style.border_left_width.px(),
    ]
    .into_iter()
    .reduce(f32::max)
    .unwrap_or(0.0);

    BoxStyle {
        background: style.background_color.map_or(Color::TRANSPARENT, rgba),
        background_image: style.background_image.clone().map(|image| match image {
            crate::ui::elements::styles::BackgroundImage::LinearGradient { direction, stops } => {
                BackgroundImage::LinearGradient {
                    direction,
                    stops: stops
                        .into_iter()
                        .map(|stop| GradientStop {
                            color: rgba(stop.color),
                            position: stop.position,
                        })
                        .collect(),
                }
            }
            crate::ui::elements::styles::BackgroundImage::RadialGradient { stops } => {
                BackgroundImage::RadialGradient {
                    stops: stops
                        .into_iter()
                        .map(|stop| GradientStop {
                            color: rgba(stop.color),
                            position: stop.position,
                        })
                        .collect(),
                }
            }
            crate::ui::elements::styles::BackgroundImage::Raster(image) => {
                BackgroundImage::Raster(image)
            }
        }),
        corner_radius: CornerRadius::new(
            radius(style.border_top_left_radius, width, height),
            radius(style.border_top_right_radius, width, height),
            radius(style.border_bottom_right_radius, width, height),
            radius(style.border_bottom_left_radius, width, height),
        ),
        border: (border_width > 0.0)
            .then(|| Border::new(border_width, style.border_color.map_or(Color::BLACK, rgba))),
        outline: (style.outline_width.px() > 0.0).then(|| {
            Outline::new(
                style.outline_width.px(),
                style.outline_offset.px(),
                style.outline_color.map_or(Color::BLACK, rgba),
            )
        }),
        opacity,
        transform,
        shadows: style
            .box_shadow
            .iter()
            .map(|shadow| BoxShadow {
                offset: [shadow.offset_x, shadow.offset_y],
                blur: shadow.blur,
                spread: shadow.spread,
                color: rgba(shadow.color),
                inset: shadow.inset,
            })
            .collect(),
    }
}

pub(super) fn multiply_transform(left: Transform, right: Transform) -> Transform {
    let l = left.matrix;
    let r = right.matrix;
    Transform {
        matrix: [
            l[0] * r[0] + l[2] * r[1],
            l[1] * r[0] + l[3] * r[1],
            l[0] * r[2] + l[2] * r[3],
            l[1] * r[2] + l[3] * r[3],
            l[0] * r[4] + l[2] * r[5] + l[4],
            l[1] * r[4] + l[3] * r[5] + l[5],
        ],
    }
}

pub(super) fn anchored_transform(transform: Transform, center: [f32; 2]) -> Transform {
    let [a, b, c, d, tx, ty] = transform.matrix;
    Transform {
        matrix: [
            a,
            b,
            c,
            d,
            tx + center[0] - a * center[0] - c * center[1],
            ty + center[1] - b * center[0] - d * center[1],
        ],
    }
}

pub(super) fn relative_transform(transform: Transform, center: [f32; 2]) -> Transform {
    Transform {
        matrix: relative_transform_matrix(transform, center),
    }
}

pub(super) fn relative_transform_matrix(transform: Transform, center: [f32; 2]) -> [f32; 6] {
    let [a, b, c, d, tx, ty] = transform.matrix;
    [
        a,
        b,
        c,
        d,
        a * center[0] + c * center[1] + tx - center[0],
        b * center[0] + d * center[1] + ty - center[1],
    ]
}

fn radius(value: LengthPercentageValue, width: f32, height: f32) -> f32 {
    match value {
        LengthPercentageValue::Px(value) => value,
        LengthPercentageValue::Percent(value) => width.min(height) * value / 100.0,
    }
}

pub(super) fn rgba(color: ElementColor) -> Color {
    Color::from_rgba8(color[0], color[1], color[2], color[3])
}
