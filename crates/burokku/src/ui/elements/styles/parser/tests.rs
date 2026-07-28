use taffy::AlignItems;

use crate::ui::elements::styles::{BackgroundImage, GradientStop, LengthPercentageValue, LineHeightValue, MaxSizeValue, Overflow, SizeValue, TextDecorationLineValue};

use super::*;

#[test]
fn display_defaults_to_block() {
    assert_eq!(Style::default().display, Display::Block);
}

#[test]
fn preserves_auto_and_always_visible_scroll_overflow() {
    let mut style = Style::default();
    set_style(&mut style, "overflow-x", Some("auto")).unwrap();
    set_style(&mut style, "overflow-y", Some("scroll")).unwrap();

    assert_eq!(style.overflow_x, Overflow::Auto);
    assert_eq!(style.overflow_y, Overflow::Scroll);
}

#[test]
fn accepts_common_layout_properties_and_css_units() {
    let mut style = Style::default();
    set_style(&mut style, "display", Some("flex")).unwrap();
    set_style(&mut style, "width", Some("75%")).unwrap();
    set_style(&mut style, "margin", Some("10px auto 20px")).unwrap();
    set_style(&mut style, "paddingInline", Some("4px")).unwrap_err();
    set_style(&mut style, "alignItems", Some("center")).unwrap();

    assert_eq!(style.display, Display::Flex);
    assert_eq!(style.width, SizeValue::Percent(75.0));
    assert_eq!(style.margin_top, SizeValue::Px(10.0));
    assert_eq!(style.margin_right, SizeValue::Auto);
    assert_eq!(style.margin_bottom, SizeValue::Px(20.0));
    assert_eq!(style.margin_left, SizeValue::Auto);
    assert_eq!(style.align_items, Some(AlignItems::CENTER));
}

#[test]
fn parses_typography_properties_and_font_fallbacks() {
    let mut style = Style::default();
    set_style(
        &mut style,
        "font-family",
        Some("\"Inter\", Noto Sans, sans-serif"),
    )
    .unwrap();
    set_style(&mut style, "font-style", Some("italic")).unwrap();
    set_style(&mut style, "text-align", Some("center")).unwrap();
    set_style(&mut style, "letter-spacing", Some("-0.5px")).unwrap();
    set_style(&mut style, "word-spacing", Some("3px")).unwrap();
    set_style(
        &mut style,
        "text-decoration",
        Some("underline line-through red"),
    )
    .unwrap();
    set_style(&mut style, "white-space", Some("pre-wrap")).unwrap();
    set_style(&mut style, "overflow-wrap", Some("anywhere")).unwrap();

    assert_eq!(
        style.font_families,
        Some(vec![
            "Inter".to_owned(),
            "Noto Sans".to_owned(),
            "sans-serif".to_owned()
        ])
    );
    assert_eq!(style.font_style, Some(FontStyleValue::Italic));
    assert_eq!(style.text_align, Some(TextAlignValue::Center));
    assert_eq!(style.letter_spacing, Some(LengthValue::Px(-0.5)));
    assert_eq!(style.word_spacing, Some(LengthValue::Px(3.0)));
    let decoration = style.text_decoration_line.unwrap();
    assert!(decoration.contains(TextDecorationLineValue::UNDERLINE));
    assert!(decoration.contains(TextDecorationLineValue::LINE_THROUGH));
    assert_eq!(style.text_decoration_color, Some([255, 0, 0, 255]));
    assert_eq!(style.white_space, Some(WhiteSpaceValue::PreWrap));
    assert_eq!(style.overflow_wrap, Some(OverflowWrapValue::Anywhere));
}

#[test]
fn expands_box_and_gap_shorthands() {
    let mut style = Style::default();
    set_style(&mut style, "padding", Some("1px 2px 3px 4px")).unwrap();
    set_style(&mut style, "gap", Some("8px 12px")).unwrap();
    set_style(&mut style, "border-radius", Some("2px 4px")).unwrap();

    assert_eq!(
        [
            style.padding_top,
            style.padding_right,
            style.padding_bottom,
            style.padding_left,
        ],
        [
            LengthPercentageValue::Px(1.0),
            LengthPercentageValue::Px(2.0),
            LengthPercentageValue::Px(3.0),
            LengthPercentageValue::Px(4.0),
        ]
    );
    assert_eq!(style.row_gap, LengthPercentageValue::Px(8.0));
    assert_eq!(style.column_gap, LengthPercentageValue::Px(12.0));
    assert_eq!(style.border_top_left_radius, LengthPercentageValue::Px(2.0));
    assert_eq!(
        style.border_top_right_radius,
        LengthPercentageValue::Px(4.0)
    );
    assert_eq!(
        style.border_bottom_right_radius,
        LengthPercentageValue::Px(2.0)
    );
    assert_eq!(
        style.border_bottom_left_radius,
        LengthPercentageValue::Px(4.0)
    );
}

#[test]
fn clears_properties_to_their_initial_values() {
    let mut style = Style::default();
    set_style(&mut style, "flex-shrink", Some("0")).unwrap();
    set_style(&mut style, "background-color", Some("#1234")).unwrap();
    set_style(&mut style, "z-index", Some("12")).unwrap();
    set_style(&mut style, "isolation", Some("isolate")).unwrap();
    set_style(&mut style, "flex-shrink", None).unwrap();
    set_style(&mut style, "background-color", Some("")).unwrap();
    set_style(&mut style, "z-index", None).unwrap();
    set_style(&mut style, "isolation", Some("")).unwrap();

    assert_eq!(style.flex_shrink, 1.0);
    assert_eq!(style.background_color, None);
    assert_eq!(style.z_index, ZIndex::Auto);
    assert_eq!(style.isolation, Isolation::Auto);
}

#[test]
fn parses_z_index_and_isolation_as_enums() {
    let mut style = Style::default();

    set_style(&mut style, "zIndex", Some("-3")).unwrap();
    set_style(&mut style, "isolation", Some("isolate")).unwrap();

    assert_eq!(style.z_index, ZIndex::Value(-3));
    assert_eq!(style.isolation, Isolation::Isolate);

    assert!(set_style(&mut style, "z-index", Some("1.5")).is_err());
    assert!(set_style(&mut style, "isolation", Some("true")).is_err());
}

#[test]
fn rejects_invalid_negative_box_sizes() {
    let mut style = Style::default();
    assert!(matches!(
        set_style(&mut style, "padding", Some("-1px")),
        Err(StyleError::InvalidValue(_, _))
    ));
}

#[test]
fn property_types_reject_values_outside_their_css_grammar() {
    let mut style = Style::default();

    assert!(set_style(&mut style, "padding", Some("auto")).is_err());
    assert!(set_style(&mut style, "border-width", Some("10%")).is_err());
    assert!(set_style(&mut style, "max-width", Some("auto")).is_err());

    set_style(&mut style, "max-width", Some("none")).unwrap();
    set_style(&mut style, "line-height", Some("normal")).unwrap();
    assert_eq!(style.max_width, MaxSizeValue::None);
    assert_eq!(style.line_height, Some(LineHeightValue::Normal));
}

#[test]
fn parses_functional_and_extended_named_colors() {
    let mut style = Style::default();
    set_style(&mut style, "color", Some("rgb(100% 0% 50% / 25%)")).unwrap();
    assert_eq!(style.color, Some([255, 0, 128, 64]));

    set_style(&mut style, "color", Some("rgba(12, 34, 56, 0.5)")).unwrap();
    assert_eq!(style.color, Some([12, 34, 56, 128]));

    set_style(&mut style, "color", Some("hsl(120 100% 25%)")).unwrap();
    assert_eq!(style.color, Some([0, 128, 0, 255]));

    set_style(&mut style, "color", Some("rebeccapurple")).unwrap();
    assert_eq!(style.color, Some([102, 51, 153, 255]));
    set_style(&mut style, "color", Some("lightgoldenrodyellow")).unwrap();
    assert_eq!(style.color, Some([250, 250, 210, 255]));
}

#[test]
fn parses_opacity_transform_and_shadows() {
    let mut style = Style::default();
    set_style(&mut style, "opacity", Some("0.35")).unwrap();
    set_style(
        &mut style,
        "transform",
        Some("translate(10px, 20px) scale(2) rotate(0deg)"),
    )
    .unwrap();
    set_style(
        &mut style,
        "box-shadow",
        Some("4px 6px 8px 2px rgba(0, 0, 0, 0.5)"),
    )
    .unwrap();
    set_style(&mut style, "text-shadow", Some("1px 2px 3px navy")).unwrap();

    assert_eq!(style.opacity, 0.35);
    assert_eq!(style.transform.matrix, [2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
    assert_eq!(style.box_shadow[0].spread, 2.0);
    assert_eq!(style.box_shadow[0].color, [0, 0, 0, 128]);
    assert_eq!(style.text_shadow[0].color, [0, 0, 128, 255]);
    assert!(set_style(&mut style, "opacity", Some("1.1")).is_err());
    assert!(set_style(&mut style, "text-shadow", Some("1px 2px 3px 4px red")).is_err());

    set_style(&mut style, "opacity", Some("35%")).unwrap();
    set_style(
        &mut style,
        "box-shadow",
        Some("inset 1px 2px 3px red, 4px 5px blue"),
    )
    .unwrap();
    set_style(
        &mut style,
        "text-shadow",
        Some("1px 2px red, 3px 4px 5px blue"),
    )
    .unwrap();
    assert_eq!(style.opacity, 0.35);
    assert_eq!(style.box_shadow.len(), 2);
    assert!(style.box_shadow[0].inset);
    assert!(!style.box_shadow[1].inset);
    assert_eq!(style.text_shadow.len(), 2);

    set_style(&mut style, "transform", Some("skewX(45deg)")).unwrap();
    assert!((style.transform.matrix[2] - 1.0).abs() < 0.0001);
}

#[test]
fn parses_linear_and_radial_gradient_images() {
    let mut style = Style::default();
    set_style(
        &mut style,
        "background-image",
        Some("linear-gradient(to right, red 0%, rgb(0 0 255) 100%)"),
    )
    .unwrap();
    assert_eq!(
        style.background_image,
        Some(BackgroundImage::LinearGradient {
            direction: [1.0, 0.0],
            stops: vec![
                GradientStop {
                    color: [255, 0, 0, 255],
                    position: 0.0,
                },
                GradientStop {
                    color: [0, 0, 255, 255],
                    position: 1.0,
                },
            ],
        })
    );

    set_style(
        &mut style,
        "background-image",
        Some("radial-gradient(white, transparent)"),
    )
    .unwrap();
    assert_eq!(
        style.background_image,
        Some(BackgroundImage::RadialGradient {
            stops: vec![
                GradientStop {
                    color: [255, 255, 255, 255],
                    position: 0.0,
                },
                GradientStop {
                    color: [0, 0, 0, 0],
                    position: 1.0,
                },
            ],
        })
    );

    set_style(
        &mut style,
        "background-image",
        Some("linear-gradient(red 10%, yellow, lime 70%, blue)"),
    )
    .unwrap();
    let Some(BackgroundImage::LinearGradient { stops, .. }) = style.background_image else {
        panic!("expected linear gradient");
    };
    assert_eq!(stops.len(), 4);
    for (actual, expected) in stops
        .iter()
        .map(|stop| stop.position)
        .zip([0.1, 0.4, 0.7, 1.0])
    {
        assert!((actual - expected).abs() < 0.0001);
    }
}

#[test]
fn decodes_and_caches_png_data_url_backgrounds() {
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8AAQv8BD/kD/YURmXYAAAAASUVORK5CYII=";
    let mut first = Style::default();
    let mut second = Style::default();
    set_style(
        &mut first,
        "background-image",
        Some(&format!("url(\"{PNG}\")")),
    )
    .unwrap();
    set_style(
        &mut second,
        "background-image",
        Some(&format!("url('{PNG}')")),
    )
    .unwrap();

    let Some(BackgroundImage::Raster(first)) = first.background_image else {
        panic!("expected decoded raster image");
    };
    let Some(BackgroundImage::Raster(second)) = second.background_image else {
        panic!("expected cached raster image");
    };
    assert_eq!((first.width, first.height), (2, 1));
    assert_eq!(&*first.pixels, &[255, 0, 0, 255, 0, 0, 255, 255]);
    assert!(std::sync::Arc::ptr_eq(&first.pixels, &second.pixels));
    assert!(set_style(
        &mut Style::default(),
        "background-image",
        Some("url(https://example.com/image.png)")
    )
    .is_err());
}

#[test]
fn rejects_pngs_exceeding_decoded_dimension_limits_before_pixel_allocation() {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, 4097, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![0; 4097 * 4]).unwrap();
    }
    assert!(decode_png(&encoded).is_none());
}

#[test]
fn expands_flex_shorthand_and_parses_order() {
    let mut style = Style::default();

    set_style(&mut style, "flex", Some("2")).unwrap();
    assert_eq!(
        (style.flex_grow, style.flex_shrink, style.flex_basis),
        (2.0, 1.0, SizeValue::Percent(0.0))
    );

    set_style(&mut style, "flex", Some("3 2 40%")).unwrap();
    assert_eq!(
        (style.flex_grow, style.flex_shrink, style.flex_basis),
        (3.0, 2.0, SizeValue::Percent(40.0))
    );

    set_style(&mut style, "flex", Some("none")).unwrap();
    assert_eq!(
        (style.flex_grow, style.flex_shrink, style.flex_basis),
        (0.0, 0.0, SizeValue::Auto)
    );

    set_style(&mut style, "flex", Some("25px")).unwrap();
    assert_eq!(
        (style.flex_grow, style.flex_shrink, style.flex_basis),
        (1.0, 1.0, SizeValue::Px(25.0))
    );

    set_style(&mut style, "order", Some("-12")).unwrap();
    assert_eq!(style.order, -12);
    assert!(set_style(&mut style, "order", Some("1.5")).is_err());

    set_style(&mut style, "flex", None).unwrap();
    set_style(&mut style, "order", None).unwrap();
    assert_eq!(
        (style.flex_grow, style.flex_shrink, style.flex_basis),
        (0.0, 1.0, SizeValue::Auto)
    );
    assert_eq!(style.order, 0);
}

#[test]
fn parses_grid_track_sizing_and_auto_flow_properties() {
    let mut style = Style::default();

    set_style(
        &mut style,
        "gridTemplateColumns",
        Some("[left] 80px [content] minmax(120px, 1fr) [right]"),
    )
    .unwrap();
    set_style(
        &mut style,
        "grid-template-rows",
        Some("repeat(2, minmax(20px, auto))"),
    )
    .unwrap();
    set_style(&mut style, "grid-auto-columns", Some("40px 10%")).unwrap();
    set_style(&mut style, "grid-auto-rows", Some("min-content 1fr")).unwrap();
    set_style(&mut style, "gridAutoFlow", Some("column dense")).unwrap();
    set_style(
        &mut style,
        "grid-template-areas",
        Some("\"header header\" \"sidebar main\""),
    )
    .unwrap();

    assert_eq!(
        style.grid_template_columns.as_deref(),
        Some("[left] 80px [content] minmax(120px, 1fr) [right]")
    );
    assert_eq!(
        style.grid_template_rows.as_deref(),
        Some("repeat(2, minmax(20px, auto))")
    );
    assert_eq!(style.grid_auto_columns.as_deref(), Some("40px 10%"));
    assert_eq!(style.grid_auto_rows.as_deref(), Some("min-content 1fr"));
    assert_eq!(
        style.grid_auto_flow,
        taffy::style::GridAutoFlow::ColumnDense
    );
    assert_eq!(
        style
            .grid_template_areas
            .iter()
            .map(|area| (
                area.name.as_str(),
                area.row_start,
                area.row_end,
                area.column_start,
                area.column_end,
            ))
            .collect::<Vec<_>>(),
        vec![
            ("header", 1, 2, 1, 3),
            ("sidebar", 2, 3, 1, 2),
            ("main", 2, 3, 2, 3),
        ]
    );

    assert!(set_style(&mut style, "grid-auto-flow", Some("dense row column")).is_err());
    assert!(set_style(&mut style, "grid-template-rows", Some("-1fr")).is_err());
    assert!(set_style(
        &mut style,
        "grid-template-areas",
        Some("\"broken broken\" \"broken other\"")
    )
    .is_err());

    set_style(&mut style, "grid-template-columns", Some("none")).unwrap();
    set_style(&mut style, "grid-auto-columns", None).unwrap();
    set_style(&mut style, "grid-template-areas", Some("none")).unwrap();
    assert_eq!(style.grid_template_columns, None);
    assert_eq!(style.grid_auto_columns, None);
    assert!(style.grid_template_areas.is_empty());
}

#[test]
fn parses_grid_template_and_placement_shorthands() {
    let mut style = Style::default();

    set_style(
        &mut style,
        "grid-template",
        Some("40px minmax(20px, auto) / 100px 1fr"),
    )
    .unwrap();
    assert_eq!(
        style.grid_template_rows.as_deref(),
        Some("40px minmax(20px, auto)")
    );
    assert_eq!(style.grid_template_columns.as_deref(), Some("100px 1fr"));

    set_style(
        &mut style,
        "grid-template",
        Some("\"header header\" 40px \"sidebar main\" minmax(60px, auto) / 100px 1fr"),
    )
    .unwrap();
    assert_eq!(
        style.grid_template_rows.as_deref(),
        Some("40px minmax(60px, auto)")
    );
    assert_eq!(style.grid_template_columns.as_deref(), Some("100px 1fr"));
    assert_eq!(
        style
            .grid_template_areas
            .iter()
            .map(|area| area.name.as_str())
            .collect::<Vec<_>>(),
        vec!["header", "sidebar", "main"]
    );
    set_style(&mut style, "grid-template", Some("none")).unwrap();
    assert_eq!(style.grid_template_rows, None);
    assert_eq!(style.grid_template_columns, None);
    assert!(style.grid_template_areas.is_empty());

    set_style(&mut style, "grid-row", Some("2 / span 3")).unwrap();
    set_style(&mut style, "grid-column-start", Some("content")).unwrap();
    set_style(&mut style, "grid-column-end", Some("-1")).unwrap();
    assert_eq!(style.grid_row_start.as_deref(), Some("2"));
    assert_eq!(style.grid_row_end.as_deref(), Some("span 3"));
    assert_eq!(style.grid_column_start.as_deref(), Some("content"));
    assert_eq!(style.grid_column_end.as_deref(), Some("-1"));

    set_style(&mut style, "grid-area", Some("2 / 3 / span 2 / 5")).unwrap();
    assert_eq!(style.grid_row_start.as_deref(), Some("2"));
    assert_eq!(style.grid_column_start.as_deref(), Some("3"));
    assert_eq!(style.grid_row_end.as_deref(), Some("span 2"));
    assert_eq!(style.grid_column_end.as_deref(), Some("5"));

    set_style(&mut style, "grid-area", Some("hero")).unwrap();
    assert_eq!(style.grid_row_start.as_deref(), Some("hero"));
    assert_eq!(style.grid_column_start.as_deref(), Some("hero"));
    assert_eq!(style.grid_row_end.as_deref(), Some("hero"));
    assert_eq!(style.grid_column_end.as_deref(), Some("hero"));

    assert!(set_style(&mut style, "grid-row", Some("1 / 2 / 3")).is_err());
    assert!(set_style(&mut style, "grid-area", Some("0")).is_err());
}
