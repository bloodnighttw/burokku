use render::{
    Border, BoxShadow, BoxStyle, Clip, CornerRadius, Outline, Rect, TextShadow, TextSpan,
    TextStyle, Transform,
};

pub(super) fn scaled_transform(transform: Transform, scale_factor: f32) -> Transform {
    let mut transform = transform;
    transform.matrix[4] *= scale_factor;
    transform.matrix[5] *= scale_factor;
    transform
}

pub(super) fn scaled_rect(rect: Rect, scale_factor: f32) -> Rect {
    Rect::new(
        rect.x * scale_factor,
        rect.y * scale_factor,
        rect.width * scale_factor,
        rect.height * scale_factor,
    )
}

pub(super) fn scaled_clip(clip: Clip, scale_factor: f32) -> Clip {
    let mut scaled = Clip::new(
        scaled_rect(clip.rect, scale_factor),
        CornerRadius::new(
            clip.corner_radius.top_left * scale_factor,
            clip.corner_radius.top_right * scale_factor,
            clip.corner_radius.bottom_right * scale_factor,
            clip.corner_radius.bottom_left * scale_factor,
        ),
    );
    scaled.transform = [
        clip.transform[0],
        clip.transform[1],
        clip.transform[2],
        clip.transform[3],
        clip.transform[4] * scale_factor,
        clip.transform[5] * scale_factor,
    ];
    scaled
}

pub(super) fn scaled_box_style(style: BoxStyle, scale_factor: f32) -> BoxStyle {
    BoxStyle {
        background: style.background,
        background_image: style.background_image,
        corner_radius: CornerRadius::new(
            style.corner_radius.top_left * scale_factor,
            style.corner_radius.top_right * scale_factor,
            style.corner_radius.bottom_right * scale_factor,
            style.corner_radius.bottom_left * scale_factor,
        ),
        border: style
            .border
            .map(|border| Border::new(border.width * scale_factor, border.color)),
        outline: style.outline.map(|outline| {
            Outline::new(
                outline.width * scale_factor,
                outline.offset * scale_factor,
                outline.color,
            )
        }),
        opacity: style.opacity,
        transform: scaled_transform(style.transform, scale_factor),
        shadows: style
            .shadows
            .into_iter()
            .map(|shadow| BoxShadow {
                offset: [
                    shadow.offset[0] * scale_factor,
                    shadow.offset[1] * scale_factor,
                ],
                blur: shadow.blur * scale_factor,
                spread: shadow.spread * scale_factor,
                color: shadow.color,
                inset: shadow.inset,
            })
            .collect(),
    }
}

pub(super) fn scaled_text_style(style: &TextStyle, scale_factor: f32) -> TextStyle {
    let mut style = style.clone();
    style.font_size *= scale_factor;
    style.line_height *= scale_factor;
    style.letter_spacing *= scale_factor;
    style.word_spacing *= scale_factor;
    style.transform = scaled_transform(style.transform, scale_factor);
    style.shadows = style
        .shadows
        .into_iter()
        .map(|shadow| TextShadow {
            offset: [
                shadow.offset[0] * scale_factor,
                shadow.offset[1] * scale_factor,
            ],
            blur: shadow.blur * scale_factor,
            color: shadow.color,
        })
        .collect();
    style
}

pub(super) fn scaled_text_spans(spans: &[TextSpan], scale_factor: f32) -> Vec<TextSpan> {
    spans
        .iter()
        .map(|span| TextSpan::new(&span.text, scaled_text_style(&span.style, scale_factor)))
        .collect()
}
