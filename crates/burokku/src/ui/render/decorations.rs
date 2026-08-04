use render::{
    BoxDecoration, BoxStyle, Canvas, Clip, Color, DecorationStyle, PaintLayer, Rect,
    TextDecorationLine, TextRunMetrics, TextSpan,
};

pub(super) fn paint_box_decorations(
    bounds: Rect,
    style: BoxStyle,
    layer: PaintLayer,
    clips: impl IntoIterator<Item = Clip> + Clone,
    canvas: &mut Canvas,
) {
    let decoration_style = DecorationStyle {
        corner_radius: style.corner_radius,
        opacity: style.opacity,
        transform: style.transform,
    };
    for shadow in style.shadows.iter().rev().filter(|shadow| !shadow.inset) {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Shadow(*shadow),
            decoration_style,
            clips.clone(),
        );
    }
    if style.background != Color::TRANSPARENT || style.background_image.is_some() {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Background {
                color: style.background,
                image: style.background_image,
            },
            decoration_style,
            clips.clone(),
        );
    }
    for shadow in style.shadows.iter().rev().filter(|shadow| shadow.inset) {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Shadow(*shadow),
            decoration_style,
            clips.clone(),
        );
    }
    if let Some(border) = style.border {
        canvas.draw_decoration_with_clips(
            layer,
            bounds,
            BoxDecoration::Border(border),
            decoration_style,
            clips.clone(),
        );
    }
    if let Some(outline) = style.outline {
        canvas.draw_decoration_with_clips(
            PaintLayer::Outline,
            bounds,
            BoxDecoration::Outline(outline),
            decoration_style,
            clips,
        );
    }
}

pub(super) fn paint_text_decorations(
    bounds: Rect,
    spans: &[TextSpan],
    runs: &[TextRunMetrics],
    scale_factor: f32,
    line_through: bool,
    clips: impl IntoIterator<Item = Clip> + Clone,
    canvas: &mut Canvas,
) {
    for run in runs {
        let Some(style) = spans.get(run.span_index).map(|span| &span.style) else {
            continue;
        };
        if style.text_decoration_line == TextDecorationLine::NONE {
            continue;
        }
        let decoration_color = style.text_decoration_color;
        let decorations = if line_through {
            [
                style
                    .text_decoration_line
                    .contains(TextDecorationLine::LINE_THROUGH)
                    .then_some((run.line_through_y, run.line_through_thickness)),
                None,
            ]
        } else {
            [
                style
                    .text_decoration_line
                    .contains(TextDecorationLine::OVERLINE)
                    .then_some((run.overline_y, run.overline_thickness)),
                style
                    .text_decoration_line
                    .contains(TextDecorationLine::UNDERLINE)
                    .then_some((run.underline_y, run.underline_thickness)),
            ]
        };
        for decoration in decorations.into_iter().flatten() {
            let (y, thickness) = decoration;
            canvas.draw_decoration_with_clips(
                if line_through {
                    PaintLayer::ContentAfterText
                } else {
                    PaintLayer::ContentBeforeText
                },
                Rect::new(
                    bounds.x + run.left * scale_factor,
                    bounds.y + y * scale_factor,
                    run.width * scale_factor,
                    thickness * scale_factor,
                ),
                BoxDecoration::Background {
                    color: decoration_color,
                    image: None,
                },
                DecorationStyle::default(),
                clips.clone(),
            );
        }
    }
}
