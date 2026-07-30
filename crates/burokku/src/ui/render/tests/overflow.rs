use super::*;

#[test]
fn hidden_and_clip_overflow_clip_descendants_to_the_padding_box() {
    for overflow in ["hidden", "clip"] {
        let mut document = Document::new();
        let container = document.create_node(ElementKind::Div);
        let overflowing = document.create_node(ElementKind::Div);
        document
            .set_style(container, "width", Some("100px"))
            .unwrap();
        document
            .set_style(container, "height", Some("40px"))
            .unwrap();
        document
            .set_style(container, "border-width", Some("2px"))
            .unwrap();
        document
            .set_style(container, "border-radius", Some("12px"))
            .unwrap();
        document
            .set_style(container, "overflow", Some(overflow))
            .unwrap();
        document
            .set_style(overflowing, "width", Some("180px"))
            .unwrap();
        document
            .set_style(overflowing, "height", Some("80px"))
            .unwrap();
        document
            .set_style(overflowing, "background-color", Some("#ff0000"))
            .unwrap();
        document
            .set_style(overflowing, "z-index", Some("10"))
            .unwrap();
        document.insert(BODY_ID, container, None).unwrap();
        document.insert(container, overflowing, None).unwrap();

        let frame = build_frame(&document, 300.0, 200.0, 2.0, &mut TextSystem::new());
        let container_layout = &frame.layout.children()[0];
        let overflowing_layout = &container_layout.children()[0];
        let expected_clip = Rect::new(
            container_layout.x + 2.0,
            container_layout.y + 2.0,
            container_layout.width - 4.0,
            container_layout.height - 4.0,
        );

        let expected_clip = Clip::new(expected_clip, CornerRadius::all(10.0));
        assert_eq!(overflowing_layout.clips, [expected_clip]);
        let clips = ordered_commands(&frame.canvas)
            .into_iter()
            .find_map(|command| match command {
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    clips,
                    ..
                } if *color == Color::from_rgba8(0xff, 0, 0, 0xff) => Some(clips),
                _ => None,
            })
            .expect("overflowing child background");
        assert_eq!(*clips, [scaled_clip(expected_clip, 2.0)]);

        let outside_container = (
            container_layout.x + container_layout.width + 10.0,
            container_layout.y + 10.0,
        );
        assert_ne!(
            frame
                .layout
                .hit_test(outside_container.0, outside_container.1)
                .map(Layout::element_id),
            Some(overflowing)
        );
        let rounded_corner = (expected_clip.rect.x + 1.0, expected_clip.rect.y + 1.0);
        assert_ne!(
            frame
                .layout
                .hit_test(rounded_corner.0, rounded_corner.1)
                .map(Layout::element_id),
            Some(overflowing)
        );
    }
}

#[test]
fn overflow_axis_only_clips_that_axis() {
    let mut document = Document::new();
    let container = document.create_node(ElementKind::Div);
    let overflowing = document.create_node(ElementKind::Div);
    document
        .set_style(container, "width", Some("100px"))
        .unwrap();
    document
        .set_style(container, "height", Some("40px"))
        .unwrap();
    document
        .set_style(container, "overflow-x", Some("hidden"))
        .unwrap();
    document
        .set_style(overflowing, "width", Some("180px"))
        .unwrap();
    document
        .set_style(overflowing, "height", Some("80px"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, overflowing, None).unwrap();

    let layout = compute_layout(&document, 300.0, 200.0, &mut TextSystem::new());
    let container_layout = &layout.children()[0];
    let clip = container_layout.children()[0].clips[0].rect;

    assert_eq!(clip.x, container_layout.x);
    assert_eq!(clip.width, container_layout.width);
    assert_eq!((clip.y, clip.height), (0.0, 200.0));
}

#[test]
fn scroll_overflow_offsets_content_and_builds_proportional_scrollbars() {
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
    document.set_style(content, "width", Some("240px")).unwrap();
    document
        .set_style(content, "height", Some("180px"))
        .unwrap();
    document
        .set_style(content, "background-color", Some("#ff0000"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, content, None).unwrap();

    let offsets = HashMap::from([(container, ScrollOffset::new(50.0, 40.0))]);
    let frame = build_frame_with_scroll(
        &document,
        300.0,
        200.0,
        1.0,
        &mut TextSystem::new(),
        &offsets,
    );
    let container_layout = &frame.layout.children()[0];
    let content_layout = &container_layout.children()[0];
    let scroll = container_layout.scroll.expect("scroll container");

    assert_eq!(scroll.max_offset, ScrollOffset::new(140.0, 120.0));
    assert_eq!(scroll.offset, ScrollOffset::new(50.0, 40.0));
    assert_eq!(
        (content_layout.x, content_layout.y),
        (container_layout.x - 50.0, container_layout.y - 40.0)
    );
    let horizontal = scroll.horizontal.expect("horizontal scrollbar");
    let vertical = scroll.vertical.expect("vertical scrollbar");
    assert!(horizontal.thumb.width < horizontal.track.width);
    assert!(vertical.thumb.height < vertical.track.height);
    assert!(horizontal.thumb.x > horizontal.track.x);
    assert!(vertical.thumb.y > vertical.track.y);

    let scrollbar_boxes = frame
        .canvas
        .commands()
        .iter()
        .filter(|command| {
            matches!(
                command,
                render::DrawCommand::Decoration {
                    layer: PaintLayer::Scrollbar,
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(15, 23, 42, 36)
                    || *color == Color::from_rgba8(71, 85, 105, 210)
            )
        })
        .count();
    assert_eq!(scrollbar_boxes, 4);
}

#[test]
fn root_scrollbar_paints_above_document_positioned_content() {
    let mut document = Document::new();
    let content = document.create_node(ElementKind::Div);
    let absolute = document.create_node(ElementKind::Div);
    let fixed = document.create_node(ElementKind::Div);
    document
        .set_style(BODY_ID, "overflow", Some("auto"))
        .unwrap();
    document.set_style(content, "width", Some("200px")).unwrap();
    document
        .set_style(content, "height", Some("200px"))
        .unwrap();
    document
        .set_style(absolute, "position", Some("absolute"))
        .unwrap();
    document.set_style(absolute, "right", Some("0px")).unwrap();
    document.set_style(absolute, "bottom", Some("0px")).unwrap();
    document.set_style(absolute, "width", Some("40px")).unwrap();
    document
        .set_style(absolute, "height", Some("40px"))
        .unwrap();
    document
        .set_style(absolute, "background-color", Some("#0000ff"))
        .unwrap();
    document
        .set_style(fixed, "position", Some("fixed"))
        .unwrap();
    document.set_style(fixed, "right", Some("0px")).unwrap();
    document.set_style(fixed, "bottom", Some("0px")).unwrap();
    document.set_style(fixed, "width", Some("40px")).unwrap();
    document.set_style(fixed, "height", Some("40px")).unwrap();
    document
        .set_style(fixed, "background-color", Some("#ff0000"))
        .unwrap();
    document.insert(BODY_ID, content, None).unwrap();
    document.insert(BODY_ID, absolute, None).unwrap();
    document.insert(BODY_ID, fixed, None).unwrap();

    let frame = build_frame(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
    assert!(
        frame
            .layout
            .scroll
            .is_some_and(|scroll| scroll.vertical.is_some()),
        "the root must have a vertical scrollbar for this ordering test"
    );

    let commands = ordered_commands(&frame.canvas);
    let absolute_index = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(0, 0, 255, 255)
            )
        })
        .expect("absolute background paint command");
    let scrollbar_index = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::Scrollbar,
                    ..
                }
            )
        })
        .expect("scrollbar paint command");
    let fixed_index = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(255, 0, 0, 255)
            )
        })
        .expect("fixed background paint command");

    assert!(
        absolute_index < fixed_index && fixed_index < scrollbar_index,
        "tree-ordered positioned content must paint below the root scrollbar"
    );
}

#[test]
fn positioned_scroller_paints_its_scrollbar_inside_its_atomic_context() {
    let mut document = Document::new();
    let fixed = document.create_node(ElementKind::Div);
    let content = document.create_node(ElementKind::Div);
    document
        .set_style(fixed, "position", Some("fixed"))
        .unwrap();
    document.set_style(fixed, "width", Some("100px")).unwrap();
    document.set_style(fixed, "height", Some("60px")).unwrap();
    document.set_style(fixed, "overflow", Some("auto")).unwrap();
    document
        .set_style(fixed, "background-color", Some("#ff0000"))
        .unwrap();
    document.set_style(content, "width", Some("100px")).unwrap();
    document
        .set_style(content, "height", Some("200px"))
        .unwrap();
    document.insert(BODY_ID, fixed, None).unwrap();
    document.insert(fixed, content, None).unwrap();

    let frame = build_frame(&document, 300.0, 200.0, 1.0, &mut TextSystem::new());
    assert!(
        !frame.canvas.commands().iter().any(|command| matches!(
            command,
            DrawCommand::Decoration {
                layer: PaintLayer::Scrollbar,
                ..
            }
        )),
        "an atomic scroller must not leak its scrollbar into the parent canvas"
    );

    let group = frame
        .canvas
        .commands()
        .iter()
        .find_map(|command| match command {
            DrawCommand::Group { canvas, .. }
                if canvas.commands().iter().any(|command| {
                    matches!(
                        command,
                        DrawCommand::Decoration {
                            layer: PaintLayer::Scrollbar,
                            ..
                        }
                    )
                }) =>
            {
                Some(canvas)
            }
            _ => None,
        })
        .expect("positioned scroller group containing its scrollbar");
    let commands = ordered_commands(group);
    let background_index = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(255, 0, 0, 255)
            )
        })
        .expect("positioned scroller background");
    let scrollbar_index = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::Scrollbar,
                    ..
                }
            )
        })
        .expect("positioned scroller scrollbar");

    assert!(background_index < scrollbar_index);
}

#[test]
fn scroll_offsets_are_clamped_when_content_shrinks() {
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
    document.set_style(content, "width", Some("120px")).unwrap();
    document.set_style(content, "height", Some("80px")).unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, content, None).unwrap();

    let offsets = HashMap::from([(container, ScrollOffset::new(500.0, 500.0))]);
    let frame = build_frame_with_scroll(
        &document,
        300.0,
        200.0,
        1.0,
        &mut TextSystem::new(),
        &offsets,
    );
    let scroll = frame.layout.children()[0].scroll.expect("scroll container");

    assert_eq!(scroll.max_offset, ScrollOffset::new(20.0, 20.0));
    assert_eq!(scroll.offset, scroll.max_offset);
}

#[test]
fn scroll_always_shows_tracks_while_auto_only_shows_them_for_overflow() {
    let mut document = Document::new();
    let automatic = document.create_node(ElementKind::Div);
    let forced = document.create_node(ElementKind::Div);
    for (element, overflow) in [(automatic, "auto"), (forced, "scroll")] {
        document.set_style(element, "width", Some("100px")).unwrap();
        document.set_style(element, "height", Some("60px")).unwrap();
        document
            .set_style(element, "overflow", Some(overflow))
            .unwrap();
        document.insert(BODY_ID, element, None).unwrap();
    }

    let frame = build_frame(&document, 300.0, 200.0, 1.0, &mut TextSystem::new());
    let automatic = frame.layout.children()[0].scroll.expect("auto container");
    let forced = frame.layout.children()[1].scroll.expect("scroll container");

    assert!(automatic.horizontal.is_none() && automatic.vertical.is_none());
    assert!(forced.horizontal.is_some() && forced.vertical.is_some());
}

#[test]
fn retained_scroll_update_matches_a_full_layout_rebuild() {
    let mut document = Document::new();
    let container = document.create_node(ElementKind::Div);
    let content = document.create_node(ElementKind::Div);
    let child = document.create_node(ElementKind::Div);
    document
        .set_style(container, "width", Some("100px"))
        .unwrap();
    document
        .set_style(container, "height", Some("60px"))
        .unwrap();
    document
        .set_style(container, "overflow", Some("auto"))
        .unwrap();
    document.set_style(content, "width", Some("240px")).unwrap();
    document
        .set_style(content, "height", Some("180px"))
        .unwrap();
    document
        .set_style(content, "overflow", Some("auto"))
        .unwrap();
    document.set_style(child, "width", Some("400px")).unwrap();
    document.set_style(child, "height", Some("300px")).unwrap();
    document
        .set_style(child, "background-color", Some("#ff0000"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, content, None).unwrap();
    document.insert(content, child, None).unwrap();

    let mut retained = build_frame(&document, 300.0, 200.0, 2.0, &mut TextSystem::new());
    let outer_offset = ScrollOffset::new(50.0, 40.0);
    let inner_offset = ScrollOffset::new(60.0, 50.0);
    assert!(retained.layout.apply_scroll_offset(container, outer_offset));
    assert!(retained.layout.apply_scroll_offset(content, inner_offset));
    repaint_frame(&mut retained, 2.0);

    let expected = build_frame_with_scroll(
        &document,
        300.0,
        200.0,
        2.0,
        &mut TextSystem::new(),
        &HashMap::from([(container, outer_offset), (content, inner_offset)]),
    );
    assert_eq!(retained, expected);
}

#[test]
fn canvas_culls_boxes_outside_the_scroll_clip() {
    let mut document = Document::new();
    let container = document.create_node(ElementKind::Div);
    document
        .set_style(container, "display", Some("flex"))
        .unwrap();
    document
        .set_style(container, "flex-direction", Some("column"))
        .unwrap();
    document
        .set_style(container, "width", Some("100px"))
        .unwrap();
    document
        .set_style(container, "height", Some("60px"))
        .unwrap();
    document
        .set_style(container, "overflow", Some("auto"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();

    for color in ["#ff0000", "#00ff00", "#0000ff", "#000000"] {
        let child = document.create_node(ElementKind::Div);
        document.set_style(child, "width", Some("100px")).unwrap();
        document.set_style(child, "height", Some("40px")).unwrap();
        document.set_style(child, "flex-shrink", Some("0")).unwrap();
        document
            .set_style(child, "background-color", Some(color))
            .unwrap();
        document.insert(container, child, None).unwrap();
    }

    let frame = build_frame_with_scroll(
        &document,
        300.0,
        200.0,
        1.0,
        &mut TextSystem::new(),
        &HashMap::from([(container, ScrollOffset::new(0.0, 80.0))]),
    );
    let backgrounds = background_colors(&frame.canvas);

    assert!(!backgrounds.contains(&Color::from_rgba8(0xff, 0, 0, 0xff)));
    assert!(!backgrounds.contains(&Color::from_rgba8(0, 0xff, 0, 0xff)));
    assert!(backgrounds.contains(&Color::from_rgba8(0, 0, 0xff, 0xff)));
}

#[test]
#[ignore = "known bug: transformed scrollbar culling mixes local and world coordinate spaces"]
fn transformed_scrollbars_remain_painted_when_the_transform_makes_them_visible() {
    let mut document = Document::new();
    let container = document.create_node(ElementKind::Div);
    let content = document.create_node(ElementKind::Div);
    for (property, value) in [
        ("position", "absolute"),
        ("left", "400px"),
        ("top", "0px"),
        ("width", "100px"),
        ("height", "60px"),
        ("overflow", "scroll"),
        ("transform", "translateX(-400px)"),
    ] {
        document
            .set_style(container, property, Some(value))
            .unwrap();
    }
    document.set_style(content, "width", Some("240px")).unwrap();
    document
        .set_style(content, "height", Some("180px"))
        .unwrap();
    document.insert(BODY_ID, container, None).unwrap();
    document.insert(container, content, None).unwrap();

    let frame = build_frame(&document, 300.0, 200.0, 1.0, &mut TextSystem::new());
    let scrollbar_commands = ordered_commands(&frame.canvas)
        .into_iter()
        .filter(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::Scrollbar,
                    ..
                }
            )
        })
        .count();

    assert_eq!(
        scrollbar_commands, 4,
        "both tracks and both thumbs should paint after translation into the viewport"
    );
}
