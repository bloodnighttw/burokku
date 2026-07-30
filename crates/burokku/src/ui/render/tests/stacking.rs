use super::*;

#[test]
fn canvas_commands_follow_stacking_context_order() {
    let mut document = Document::new();
    let ordinary = document.create_node(ElementKind::Div);
    let high_descendant = document.create_node(ElementKind::Div);
    let middle = document.create_node(ElementKind::Div);
    let negative = document.create_node(ElementKind::Div);

    for (element, color) in [
        (ordinary, "#ff0000"),
        (high_descendant, "#00ff00"),
        (middle, "#0000ff"),
        (negative, "#000000"),
    ] {
        document
            .set_style(element, "background-color", Some(color))
            .unwrap();
        document.set_style(element, "width", Some("20px")).unwrap();
        document.set_style(element, "height", Some("20px")).unwrap();
    }
    document
        .set_style(high_descendant, "z-index", Some("10"))
        .unwrap();
    document.set_style(middle, "z-index", Some("5")).unwrap();
    document.set_style(negative, "z-index", Some("-1")).unwrap();
    for element in [high_descendant, middle, negative] {
        document
            .set_style(element, "position", Some("relative"))
            .unwrap();
    }

    document.insert(BODY_ID, ordinary, None).unwrap();
    document.insert(ordinary, high_descendant, None).unwrap();
    document.insert(BODY_ID, middle, None).unwrap();
    document.insert(BODY_ID, negative, None).unwrap();

    let canvas = build_canvas(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
    let backgrounds = background_colors(&canvas);

    assert_eq!(
        backgrounds,
        [
            Color::BLACK,
            Color::from_rgba8(0xff, 0, 0, 0xff),
            Color::from_rgba8(0, 0, 0xff, 0xff),
            Color::from_rgba8(0, 0xff, 0, 0xff),
        ]
    );
}

#[test]
fn z_index_levels_follow_css_paint_order_and_preserve_tree_order_ties() {
    let mut document = Document::new();
    let positive_high = document.create_node(ElementKind::Div);
    let zero_early = document.create_node(ElementKind::Div);
    let negative_near = document.create_node(ElementKind::Div);
    let ordinary = document.create_node(ElementKind::Div);
    let positioned_auto = document.create_node(ElementKind::Div);
    let positive_tie_first = document.create_node(ElementKind::Div);
    let negative_far = document.create_node(ElementKind::Div);
    let zero_late = document.create_node(ElementKind::Div);
    let positive_low = document.create_node(ElementKind::Div);
    let positive_tie_second = document.create_node(ElementKind::Div);

    let elements = [
        (positive_high, "#a00000", Some("10")),
        (zero_early, "#00a0a0", Some("0")),
        (negative_near, "#202020", Some("-1")),
        (ordinary, "#a0a0a0", None),
        (positioned_auto, "#a0a000", None),
        (positive_tie_first, "#00a000", Some("2")),
        (negative_far, "#000000", Some("-10")),
        (zero_late, "#0000a0", Some("0")),
        (positive_low, "#a000a0", Some("1")),
        (positive_tie_second, "#00a0ff", Some("2")),
    ];

    for (element, color, z_index) in elements {
        document
            .set_style(element, "background-color", Some(color))
            .unwrap();
        document.set_style(element, "width", Some("40px")).unwrap();
        document.set_style(element, "height", Some("40px")).unwrap();
        if element != ordinary {
            document
                .set_style(element, "position", Some("absolute"))
                .unwrap();
        }
        if let Some(z_index) = z_index {
            document
                .set_style(element, "z-index", Some(z_index))
                .unwrap();
        }
        document.insert(BODY_ID, element, None).unwrap();
    }

    let frame = build_frame(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());

    assert_eq!(
        background_colors(&frame.canvas),
        [
            Color::from_rgba8(0x00, 0x00, 0x00, 0xff),
            Color::from_rgba8(0x20, 0x20, 0x20, 0xff),
            Color::from_rgba8(0xa0, 0xa0, 0xa0, 0xff),
            Color::from_rgba8(0x00, 0xa0, 0xa0, 0xff),
            Color::from_rgba8(0xa0, 0xa0, 0x00, 0xff),
            Color::from_rgba8(0x00, 0x00, 0xa0, 0xff),
            Color::from_rgba8(0xa0, 0x00, 0xa0, 0xff),
            Color::from_rgba8(0x00, 0xa0, 0x00, 0xff),
            Color::from_rgba8(0x00, 0xa0, 0xff, 0xff),
            Color::from_rgba8(0xa0, 0x00, 0x00, 0xff),
        ]
    );
    assert_eq!(
        frame
            .layout
            .iter()
            .map(Layout::element_id)
            .collect::<Vec<_>>(),
        [
            BODY_ID,
            negative_far,
            negative_near,
            ordinary,
            zero_early,
            positioned_auto,
            zero_late,
            positive_low,
            positive_tie_first,
            positive_tie_second,
            positive_high,
        ]
    );
    assert_eq!(
        frame.layout.hit_test(1.0, 1.0).map(Layout::element_id),
        Some(positive_high)
    );
}

#[test]
fn viewport_fixed_contexts_obey_numeric_z_index_and_hit_testing() {
    let mut document = Document::new();
    let absolute = document.create_node(ElementKind::Div);
    let fixed = document.create_node(ElementKind::Div);
    for element in [absolute, fixed] {
        document.set_style(element, "left", Some("0px")).unwrap();
        document.set_style(element, "top", Some("0px")).unwrap();
        document.set_style(element, "width", Some("40px")).unwrap();
        document.set_style(element, "height", Some("40px")).unwrap();
    }
    document
        .set_style(absolute, "position", Some("absolute"))
        .unwrap();
    document
        .set_style(absolute, "z-index", Some("100"))
        .unwrap();
    document
        .set_style(absolute, "background-color", Some("blue"))
        .unwrap();
    document
        .set_style(fixed, "position", Some("fixed"))
        .unwrap();
    document
        .set_style(fixed, "background-color", Some("red"))
        .unwrap();
    document.insert(BODY_ID, absolute, None).unwrap();
    document.insert(BODY_ID, fixed, None).unwrap();

    let frame = build_frame(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
    assert_eq!(
        background_colors(&frame.canvas),
        [
            Color::from_rgba8(255, 0, 0, 255),
            Color::from_rgba8(0, 0, 255, 255)
        ]
    );
    assert_eq!(
        frame.layout.hit_test(10.0, 10.0).map(Layout::element_id),
        Some(absolute)
    );
}

#[test]
fn auto_z_flex_items_paint_ordinary_contents_atomically() {
    let mut document = Document::new();
    let row = document.create_node(ElementKind::Div);
    let first = document.create_node(ElementKind::Div);
    let first_text = document.create_node(ElementKind::Text("first".into()));
    let escaped_high = document.create_node(ElementKind::Div);
    let second = document.create_node(ElementKind::Div);
    let second_text = document.create_node(ElementKind::Text("second".into()));
    let escaped_middle = document.create_node(ElementKind::Div);

    document.set_style(row, "display", Some("flex")).unwrap();
    for (item, color) in [(first, "red"), (second, "blue")] {
        document.set_style(item, "width", Some("50px")).unwrap();
        document.set_style(item, "height", Some("30px")).unwrap();
        document
            .set_style(item, "background-color", Some(color))
            .unwrap();
    }
    for (context, index, color) in [
        (escaped_high, "10", "green"),
        (escaped_middle, "5", "black"),
    ] {
        document
            .set_style(context, "position", Some("relative"))
            .unwrap();
        document.set_style(context, "z-index", Some(index)).unwrap();
        document
            .set_style(context, "background-color", Some(color))
            .unwrap();
        document.set_style(context, "width", Some("10px")).unwrap();
        document.set_style(context, "height", Some("10px")).unwrap();
    }
    document.insert(BODY_ID, row, None).unwrap();
    document.insert(row, first, None).unwrap();
    document.insert(first, first_text, None).unwrap();
    document.insert(first, escaped_high, None).unwrap();
    document.insert(row, second, None).unwrap();
    document.insert(second, second_text, None).unwrap();
    document.insert(second, escaped_middle, None).unwrap();

    let frame = build_frame(&document, 120.0, 60.0, 1.0, &mut TextSystem::new());
    let commands = ordered_commands(&frame.canvas);
    let red = commands
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
        .unwrap();
    let first_text_index = commands
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { text, .. } if text == "first"))
        .unwrap();
    let blue = commands
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
        .unwrap();
    let black = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::BLACK
            )
        })
        .unwrap();
    let green = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(0, 128, 0, 255)
            )
        })
        .unwrap();

    assert!(red < first_text_index && first_text_index < blue);
    assert!(blue < black && black < green);
    assert_eq!(
        frame
            .layout
            .iter()
            .map(Layout::element_id)
            .collect::<Vec<_>>(),
        [
            BODY_ID,
            row,
            first,
            first_text,
            second,
            second_text,
            escaped_middle,
            escaped_high
        ]
    );
    assert_eq!(
        frame
            .layout
            .iter_rev()
            .map(Layout::element_id)
            .collect::<Vec<_>>(),
        [
            escaped_high,
            escaped_middle,
            second_text,
            second,
            first_text,
            first,
            row,
            BODY_ID
        ]
    );
}

#[test]
fn earlier_ordinary_text_paints_before_a_later_overlapping_flex_item() {
    let mut document = Document::new();
    let first = document.create_node(ElementKind::Div);
    let first_text = document.create_node(ElementKind::Text("earlier".into()));
    let row = document.create_node(ElementKind::Div);
    let item = document.create_node(ElementKind::Div);

    document.set_style(first, "height", Some("30px")).unwrap();
    document.set_style(row, "display", Some("flex")).unwrap();
    document
        .set_style(row, "margin-top", Some("-30px"))
        .unwrap();
    document.set_style(item, "width", Some("80px")).unwrap();
    document.set_style(item, "height", Some("30px")).unwrap();
    document
        .set_style(item, "background-color", Some("blue"))
        .unwrap();
    document.insert(BODY_ID, first, None).unwrap();
    document.insert(first, first_text, None).unwrap();
    document.insert(BODY_ID, row, None).unwrap();
    document.insert(row, item, None).unwrap();

    let frame = build_frame(&document, 100.0, 60.0, 1.0, &mut TextSystem::new());
    let first_layout = frame
        .layout
        .iter()
        .find(|layout| layout.element_id == first_text)
        .unwrap();
    let item_layout = frame
        .layout
        .iter()
        .find(|layout| layout.element_id == item)
        .unwrap();
    assert!(first_layout.y < item_layout.y + item_layout.height);
    assert!(item_layout.y < first_layout.y + first_layout.height);

    let commands = ordered_commands(&frame.canvas);
    let text = commands
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { text, .. } if text == "earlier"))
        .unwrap();
    let item_background = commands
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
        .unwrap();
    assert!(text < item_background);

    let all_forward = frame
        .layout
        .iter()
        .map(Layout::element_id)
        .collect::<Vec<_>>();
    let all_reverse = frame
        .layout
        .iter_rev()
        .map(Layout::element_id)
        .collect::<Vec<_>>();
    assert_eq!(
        all_reverse,
        all_forward.iter().rev().copied().collect::<Vec<_>>()
    );

    let forward = all_forward
        .iter()
        .copied()
        .filter_map(|layout| [first_text, item].contains(&layout).then_some(layout))
        .collect::<Vec<_>>();
    let reverse = all_reverse
        .iter()
        .copied()
        .filter_map(|layout| [first_text, item].contains(&layout).then_some(layout))
        .collect::<Vec<_>>();
    assert_eq!(forward, [first_text, item]);
    assert_eq!(reverse, forward.iter().rev().copied().collect::<Vec<_>>());

    let overlap_x = first_layout.x.max(item_layout.x) + 1.0;
    let overlap_y = first_layout.y.max(item_layout.y) + 1.0;
    assert_eq!(
        frame
            .layout
            .hit_test(overlap_x, overlap_y)
            .map(Layout::element_id),
        Some(item)
    );
}

#[test]
fn overlapping_decorated_text_layouts_keep_tree_order_atomically() {
    let mut document = Document::new();
    let first = document.create_node(ElementKind::Div);
    let first_text = document.create_node(ElementKind::Text("first".into()));
    let second = document.create_node(ElementKind::Div);
    let second_text = document.create_node(ElementKind::Text("second".into()));

    for element in [first, second] {
        document.set_style(element, "height", Some("30px")).unwrap();
    }
    document
        .set_style(first, "text-decoration", Some("line-through red"))
        .unwrap();
    document
        .set_style(second, "text-decoration", Some("underline blue"))
        .unwrap();
    document
        .set_style(second, "margin-top", Some("-30px"))
        .unwrap();
    document.insert(BODY_ID, first, None).unwrap();
    document.insert(first, first_text, None).unwrap();
    document.insert(BODY_ID, second, None).unwrap();
    document.insert(second, second_text, None).unwrap();

    let frame = build_frame(&document, 100.0, 60.0, 1.0, &mut TextSystem::new());
    let layouts = [first_text, second_text].map(|element_id| {
        frame
            .layout
            .iter()
            .find(|layout| layout.element_id == element_id)
            .unwrap()
    });
    assert!(layouts[0].y < layouts[1].y + layouts[1].height);
    assert!(layouts[1].y < layouts[0].y + layouts[0].height);

    let commands = ordered_commands(&frame.canvas);
    let first_glyphs = commands
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { text, .. } if text == "first"))
        .unwrap();
    let first_strike = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::ContentAfterText,
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(255, 0, 0, 255)
            )
        })
        .unwrap();
    let second_underline = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    layer: PaintLayer::ContentBeforeText,
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(0, 0, 255, 255)
            )
        })
        .unwrap();
    let second_glyphs = commands
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { text, .. } if text == "second"))
        .unwrap();

    assert!(
        first_glyphs < first_strike
            && first_strike < second_underline
            && second_underline < second_glyphs
    );
}

#[test]
fn block_decorations_paint_before_inline_text_and_outlines_paint_last() {
    let mut document = Document::new();
    let first = document.create_node(ElementKind::Div);
    let text = document.create_node(ElementKind::Text("front".into()));
    let second = document.create_node(ElementKind::Div);
    let positive = document.create_node(ElementKind::Div);

    document
        .set_style(first, "background-color", Some("red"))
        .unwrap();
    document
        .set_style(first, "outline-color", Some("black"))
        .unwrap();
    document
        .set_style(first, "outline-width", Some("2px"))
        .unwrap();
    document
        .set_style(second, "background-color", Some("blue"))
        .unwrap();
    document
        .set_style(positive, "background-color", Some("green"))
        .unwrap();
    document
        .set_style(positive, "position", Some("relative"))
        .unwrap();
    document.set_style(positive, "z-index", Some("1")).unwrap();
    for element in [first, second, positive] {
        document.set_style(element, "width", Some("40px")).unwrap();
        document.set_style(element, "height", Some("20px")).unwrap();
    }
    document.insert(BODY_ID, first, None).unwrap();
    document.insert(first, text, None).unwrap();
    document.insert(BODY_ID, second, None).unwrap();
    document.insert(BODY_ID, positive, None).unwrap();

    let canvas = build_canvas(&document, 100.0, 100.0, 1.0, &mut TextSystem::new());
    let commands = ordered_commands(&canvas);
    let red = commands
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
        .unwrap();
    let blue = commands
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
        .unwrap();
    let text = commands
        .iter()
        .position(|command| matches!(command, DrawCommand::Text { .. }))
        .unwrap();
    let green = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Background { color, .. },
                    ..
                } if *color == Color::from_rgba8(0, 128, 0, 255)
            )
        })
        .unwrap();
    let outline = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                DrawCommand::Decoration {
                    decoration: BoxDecoration::Outline(_),
                    ..
                }
            )
        })
        .unwrap();

    assert!(red < text);
    assert!(blue < text);
    assert!(text < green);
    assert!(green < outline);
}
