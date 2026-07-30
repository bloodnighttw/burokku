use super::*;

#[test]
fn builds_canvas_from_computed_ui_layout() {
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    let text = document.create_node(ElementKind::Text("Hello UI".into()));
    document.set_style(card, "width", Some("300px")).unwrap();
    document
        .set_style(card, "background-color", Some("#f5f7fa"))
        .unwrap();
    document.set_style(card, "color", Some("#102030")).unwrap();
    document.insert(BODY_ID, card, None).unwrap();
    document.insert(card, text, None).unwrap();

    let canvas = build_canvas(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());

    assert_eq!(canvas.commands().len(), 2);
}

#[test]
fn frame_preserves_the_layout_used_to_build_the_canvas() {
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    document.set_style(card, "width", Some("100px")).unwrap();
    document.set_style(card, "height", Some("50px")).unwrap();
    document
        .set_style(card, "background-color", Some("#ffffff"))
        .unwrap();
    document.insert(BODY_ID, card, None).unwrap();

    let frame = build_frame(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());

    assert_eq!(frame.layout.kind.children()[0].element_id(), card);
    assert_eq!(frame.canvas.commands().len(), 1);
}

#[test]
fn carries_paint_effects_from_style_to_canvas_commands() {
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    document.set_style(card, "width", Some("100px")).unwrap();
    document.set_style(card, "height", Some("50px")).unwrap();
    document.set_style(card, "opacity", Some("0.5")).unwrap();
    document
        .set_style(card, "transform", Some("translate(3px, 4px)"))
        .unwrap();
    document
        .set_style(card, "box-shadow", Some("1px 2px 3px 4px navy"))
        .unwrap();
    document
        .set_style(
            card,
            "background-image",
            Some("linear-gradient(to right, red, blue)"),
        )
        .unwrap();
    document.insert(BODY_ID, card, None).unwrap();

    let canvas = build_canvas(&document, 200.0, 100.0, 2.0, &mut TextSystem::new());
    let DrawCommand::Group {
        canvas,
        opacity,
        transform,
        ..
    } = &canvas.commands()[0]
    else {
        panic!("expected an effect group");
    };
    assert_eq!(*opacity, 0.5);
    assert_eq!(transform.matrix[4..], [6.0, 8.0]);
    let background = canvas
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Decoration {
                decoration: BoxDecoration::Background { image, .. },
                style,
                ..
            } => Some((image, style)),
            _ => None,
        })
        .expect("grouped background decoration");
    assert_eq!(background.1.opacity, 1.0);
    assert_eq!(background.1.transform, Transform::IDENTITY);
    assert!(matches!(
        background.0,
        Some(BackgroundImage::LinearGradient { .. })
    ));
    let shadow = canvas
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Decoration {
                decoration: BoxDecoration::Shadow(shadow),
                ..
            } => Some(shadow),
            _ => None,
        })
        .expect("grouped shadow decoration");
    assert_eq!(shadow.offset, [2.0, 4.0]);
}

#[test]
fn carries_decoded_raster_backgrounds_to_canvas_commands() {
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    document.set_style(card, "width", Some("20px")).unwrap();
    document.set_style(card, "height", Some("10px")).unwrap();
    document
        .set_style(card, "background-image", Some(&format!("url({PNG})")))
        .unwrap();
    document.insert(BODY_ID, card, None).unwrap();

    let canvas = build_canvas(&document, 100.0, 50.0, 1.0, &mut TextSystem::new());
    let image = ordered_commands(&canvas)
        .into_iter()
        .find_map(|command| match command {
            DrawCommand::Decoration {
                decoration:
                    BoxDecoration::Background {
                        image: Some(BackgroundImage::Raster(image)),
                        ..
                    },
                ..
            } => Some(image),
            _ => None,
        })
        .expect("expected raster background decoration");
    assert_eq!((image.width, image.height), (2, 1));
}

#[test]
fn retains_opacity_and_transform_subtrees_as_recursive_groups() {
    let mut document = Document::new();
    let parent = document.create_node(ElementKind::Div);
    let first = document.create_node(ElementKind::Div);
    let second = document.create_node(ElementKind::Text("rotated".into()));
    document.set_style(parent, "width", Some("100px")).unwrap();
    document.set_style(parent, "height", Some("80px")).unwrap();
    document.set_style(parent, "opacity", Some("50%")).unwrap();
    document
        .set_style(parent, "transform", Some("rotate(30deg) skewX(10deg)"))
        .unwrap();
    document.set_style(first, "width", Some("40px")).unwrap();
    document.set_style(first, "height", Some("30px")).unwrap();
    document
        .set_style(first, "background-color", Some("red"))
        .unwrap();
    document.insert(BODY_ID, parent, None).unwrap();
    document.insert(parent, first, None).unwrap();
    document.insert(parent, second, None).unwrap();

    let canvas = build_canvas(&document, 200.0, 160.0, 1.0, &mut TextSystem::new());
    let DrawCommand::Group {
        canvas,
        opacity,
        transform,
        origin,
        ..
    } = &canvas.commands()[0]
    else {
        panic!("expected recursive effect group");
    };
    assert_eq!(*opacity, 0.5);
    assert_ne!(*transform, Transform::IDENTITY);
    assert_eq!(*origin, [50.0, 40.0]);
    assert!(ordered_commands(canvas)
        .iter()
        .any(|command| matches!(command, DrawCommand::Text { .. })));
    assert!(canvas
        .commands()
        .iter()
        .any(|command| matches!(command, DrawCommand::Decoration { .. })));
}

#[test]
fn localizes_overflow_clips_inside_transformed_groups() {
    let mut document = Document::new();
    let parent = document.create_node(ElementKind::Div);
    let child = document.create_node(ElementKind::Div);
    document.set_style(parent, "width", Some("100px")).unwrap();
    document.set_style(parent, "height", Some("80px")).unwrap();
    document
        .set_style(parent, "overflow", Some("hidden"))
        .unwrap();
    document
        .set_style(parent, "transform", Some("rotate(30deg)"))
        .unwrap();
    document.set_style(child, "width", Some("120px")).unwrap();
    document.set_style(child, "height", Some("100px")).unwrap();
    document
        .set_style(child, "background-color", Some("red"))
        .unwrap();
    document.insert(BODY_ID, parent, None).unwrap();
    document.insert(parent, child, None).unwrap();

    let canvas = build_canvas(&document, 200.0, 160.0, 1.0, &mut TextSystem::new());
    let DrawCommand::Group { canvas, .. } = &canvas.commands()[0] else {
        panic!("expected transformed parent group");
    };
    let child_clip = canvas
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Decoration { clips, .. } if !clips.is_empty() => Some(clips[0]),
            _ => None,
        })
        .expect("overflow clip on child");
    for (actual, expected) in child_clip
        .transform
        .into_iter()
        .zip(Transform::IDENTITY.matrix)
    {
        assert!((actual - expected).abs() < 0.0001);
    }
}

#[test]
fn emits_text_decorations_around_the_glyph_layer() {
    let mut document = Document::new();
    let container = document.create_node(ElementKind::Div);
    let text = document.create_node(ElementKind::Text("decorated".into()));
    document
        .set_style(
            container,
            "text-decoration",
            Some("underline line-through red"),
        )
        .unwrap();
    document
        .set_style(container, "text-align", Some("right"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, text, None).unwrap();

    let frame = build_frame(&document, 200.0, 100.0, 1.0, &mut TextSystem::new());
    let canvas = &frame.canvas;
    let commands = ordered_commands(canvas);
    let decoration_layers = commands
        .iter()
        .filter_map(|command| match command {
            render::DrawCommand::Decoration { layer, .. }
                if matches!(
                    layer,
                    PaintLayer::ContentBeforeText | PaintLayer::ContentAfterText
                ) =>
            {
                Some(*layer)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        decoration_layers,
        [PaintLayer::ContentBeforeText, PaintLayer::ContentAfterText]
    );
    let LayoutKind::Text { runs, .. } = &frame.layout.children()[0].children()[0].kind else {
        panic!("child should be text");
    };
    let rect = commands
        .iter()
        .find_map(|command| match command {
            render::DrawCommand::Decoration {
                layer: PaintLayer::ContentBeforeText,
                rect,
                ..
            } => Some(rect),
            _ => None,
        })
        .expect("underline decoration before text");
    assert!((rect.x - runs[0].left).abs() < 0.01);
    assert!((rect.width - runs[0].width).abs() < 0.01);
    assert!(rect.x > 0.0);
    assert!(rect.width < 200.0);

    let before = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::ContentBeforeText,
                    ..
                }
            )
        })
        .unwrap();
    let text = commands
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { .. }))
        .unwrap();
    let after = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::ContentAfterText,
                    ..
                }
            )
        })
        .unwrap();
    assert!(before < text && text < after);
}

#[test]
fn decorates_only_the_styled_nested_text() {
    let mut document = Document::new();
    let line = document.create_node(ElementKind::TextElement);
    let before = document.create_node(ElementKind::Text("before ".into()));
    let decorated = document.create_node(ElementKind::TextElement);
    let decorated_text = document.create_node(ElementKind::Text("middle".into()));
    let after = document.create_node(ElementKind::Text(" after".into()));
    document
        .set_style(decorated, "text-decoration", Some("underline #7c3aed"))
        .unwrap();
    document.insert(BODY_ID, line, None).unwrap();
    document.insert(line, before, None).unwrap();
    document.insert(line, decorated, None).unwrap();
    document.insert(decorated, decorated_text, None).unwrap();
    document.insert(line, after, None).unwrap();

    let frame = build_frame(&document, 300.0, 100.0, 1.0, &mut TextSystem::new());
    let inline = &frame.layout.children()[0];
    let LayoutKind::Text { spans, runs, .. } = &inline.kind else {
        panic!("nested text should produce one rich text layout");
    };
    let decorated_runs = runs.iter().filter(|run| run.span_index == 1).count();
    let painted = ordered_commands(&frame.canvas);
    let decorations: Vec<_> = painted
        .iter()
        .filter_map(|command| match command {
            render::DrawCommand::Decoration {
                layer: PaintLayer::ContentBeforeText,
                decoration: BoxDecoration::Background { color, .. },
                ..
            } => Some(color),
            _ => None,
        })
        .collect();

    assert_eq!(spans.len(), 3);
    assert!(decorated_runs > 0);
    assert_eq!(decorations.len(), decorated_runs);
    assert!(decorations
        .iter()
        .all(|color| **color == Color::from_rgba8(0x7c, 0x3a, 0xed, 0xff)));
}

#[test]
#[ignore = "known bug: nested text span opacity is retained but not consumed by paint commands"]
fn nested_text_element_opacity_applies_to_its_decorations() {
    fn collect_effective_decoration_alpha(
        canvas: &Canvas,
        target: Color,
        inherited_opacity: f32,
        output: &mut Vec<f32>,
    ) {
        for command in canvas.commands() {
            match command {
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    style,
                    ..
                } if *color == target => {
                    output.push(inherited_opacity * style.opacity * color.alpha);
                }
                DrawCommand::Group {
                    canvas, opacity, ..
                } => collect_effective_decoration_alpha(
                    canvas,
                    target,
                    inherited_opacity * opacity,
                    output,
                ),
                _ => {}
            }
        }
    }

    let mut document = Document::new();
    let line = document.create_node(ElementKind::TextElement);
    let before = document.create_node(ElementKind::Text("before ".into()));
    let hidden = document.create_node(ElementKind::TextElement);
    let hidden_text = document.create_node(ElementKind::Text("hidden".into()));
    let after = document.create_node(ElementKind::Text(" after".into()));
    document.set_style(hidden, "opacity", Some("0")).unwrap();
    document
        .set_style(hidden, "text-decoration", Some("underline #7c3aed"))
        .unwrap();
    document.insert(BODY_ID, line, None).unwrap();
    document.insert(line, before, None).unwrap();
    document.insert(line, hidden, None).unwrap();
    document.insert(hidden, hidden_text, None).unwrap();
    document.insert(line, after, None).unwrap();

    let frame = build_frame(&document, 300.0, 100.0, 1.0, &mut TextSystem::new());
    let mut decoration_alpha = Vec::new();
    collect_effective_decoration_alpha(
        &frame.canvas,
        Color::from_rgba8(0x7c, 0x3a, 0xed, 0xff),
        1.0,
        &mut decoration_alpha,
    );

    assert!(
        !decoration_alpha.is_empty(),
        "the nested underlined span should produce a decoration"
    );
    assert!(
        decoration_alpha.iter().all(|alpha| *alpha <= f32::EPSILON),
        "opacity: 0 must make every nested-span decoration transparent, got {decoration_alpha:?}"
    );
}

#[test]
fn wrapped_centered_decorations_follow_each_shaped_line() {
    let mut document = Document::new();
    let container = document.create_node(ElementKind::Div);
    let text = document.create_node(ElementKind::Text("decorations follow wrapped lines".into()));
    document
        .set_style(container, "width", Some("90px"))
        .unwrap();
    document
        .set_style(container, "text-align", Some("center"))
        .unwrap();
    document
        .set_style(container, "text-decoration", Some("underline"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, text, None).unwrap();

    let frame = build_frame(&document, 200.0, 150.0, 1.0, &mut TextSystem::new());
    let text_layout = &frame.layout.children()[0].children()[0];
    let LayoutKind::Text { runs, .. } = &text_layout.kind else {
        panic!("child should be text");
    };
    let painted = ordered_commands(&frame.canvas);
    let decorations: Vec<_> = painted
        .iter()
        .filter_map(|command| match command {
            render::DrawCommand::Decoration {
                layer: PaintLayer::ContentBeforeText,
                rect,
                ..
            } => Some(rect),
            _ => None,
        })
        .collect();

    assert!(runs.len() > 1);
    assert_eq!(decorations.len(), runs.len());
    for (rect, run) in decorations.into_iter().zip(runs) {
        assert!((rect.x - (text_layout.x + run.left)).abs() < 0.01);
        assert!((rect.width - run.width).abs() < 0.01);
        assert!(rect.width < text_layout.width);
    }
}

#[test]
fn scales_geometry_and_paint_styles_for_physical_pixels() {
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    document.set_style(card, "width", Some("100px")).unwrap();
    document.set_style(card, "height", Some("50px")).unwrap();
    document
        .set_style(card, "border-width", Some("2px"))
        .unwrap();
    document
        .set_style(card, "border-radius", Some("4px"))
        .unwrap();
    document.insert(BODY_ID, card, None).unwrap();

    let canvas = build_canvas(&document, 800.0, 600.0, 2.0, &mut TextSystem::new());
    let (rect, border, style) = ordered_commands(&canvas)
        .into_iter()
        .find_map(|command| match command {
            DrawCommand::Decoration {
                rect,
                decoration: BoxDecoration::Border(border),
                style,
                ..
            } => Some((rect, border, style)),
            _ => None,
        })
        .expect("card should produce a border decoration");

    assert_eq!((rect.width, rect.height), (208.0, 108.0));
    assert_eq!(border.width, 4.0);
    assert_eq!(style.corner_radius.top_left, 8.0);
}

#[test]
#[ignore = "known bug: per-side border widths are collapsed to one uniform paint width"]
fn preserves_one_sided_border_widths_in_paint_commands() {
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    document.set_style(card, "width", Some("100px")).unwrap();
    document.set_style(card, "height", Some("50px")).unwrap();
    document
        .set_style(card, "border-left-width", Some("10px"))
        .unwrap();
    document.insert(BODY_ID, card, None).unwrap();

    let frame = build_frame(&document, 800.0, 600.0, 1.0, &mut TextSystem::new());
    let card_layout = &frame.layout.kind.children()[0];
    assert_eq!((card_layout.width, card_layout.height), (110.0, 50.0));

    let border_paints_at = |x: f32, y: f32| {
        ordered_commands(&frame.canvas)
            .into_iter()
            .any(|command| match command {
                DrawCommand::Decoration {
                    rect,
                    decoration: BoxDecoration::Border(border),
                    ..
                } if border.color == Color::BLACK && rect.contains(x, y) => {
                    x < rect.x + border.width
                        || x >= rect.x + rect.width - border.width
                        || y < rect.y + border.width
                        || y >= rect.y + rect.height - border.width
                }
                DrawCommand::Decoration {
                    rect,
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } => *color == Color::BLACK && rect.contains(x, y),
                _ => false,
            })
    };

    assert!(border_paints_at(5.0, 25.0), "left edge should be painted");
    assert!(
        !border_paints_at(55.0, 5.0),
        "top edge should remain unpainted"
    );
    assert!(
        !border_paints_at(105.0, 25.0),
        "right edge should remain unpainted"
    );
    assert!(
        !border_paints_at(55.0, 45.0),
        "bottom edge should remain unpainted"
    );
}

#[test]
#[ignore = "known bug: offset-shadow culling uses the expanded bounds center as the transform origin"]
fn transformed_offset_shadow_uses_the_element_origin_when_culling() {
    let mut document = Document::new();
    let card = document.create_node(ElementKind::Div);
    for (property, value) in [
        ("position", "absolute"),
        ("left", "200px"),
        ("top", "20px"),
        ("width", "10px"),
        ("height", "10px"),
        ("box-shadow", "100px 0 0 0 red"),
        ("transform", "rotate(180deg)"),
    ] {
        document.set_style(card, property, Some(value)).unwrap();
    }
    document.insert(BODY_ID, card, None).unwrap();

    // Rotating around the 10px box's center moves the offset shadow from
    // x=300..310 to x=100..110, which is inside this 150px viewport.
    let canvas = build_canvas(&document, 150.0, 80.0, 1.0, &mut TextSystem::new());
    assert!(
        ordered_commands(&canvas).into_iter().any(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Shadow(_),
                    ..
                }
            )
        }),
        "the visible transformed shadow must not be culled"
    );
}
