struct Screen {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> screen: Screen;

struct Clip {
    center: vec2<f32>,
    half_size: vec2<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
}

@group(0) @binding(1)
var<storage, read> clips: array<Clip>;

struct Instance {
    @location(0) center: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) radii_x: vec4<f32>,
    @location(3) radii_y: vec4<f32>,
    @location(4) background: vec4<f32>,
    @location(5) border_top_color: vec4<f32>,
    @location(6) border_right_color: vec4<f32>,
    @location(7) border_bottom_color: vec4<f32>,
    @location(8) border_left_color: vec4<f32>,
    @location(9) outline_color: vec4<f32>,
    @location(10) border_widths: vec4<f32>,
    @location(11) border_styles: vec4<u32>,
    @location(12) outline_width: f32,
    @location(13) outline_offset: f32,
    @location(14) clip_range: vec2<u32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_position: vec2<f32>,
    @location(1) @interpolate(flat) half_size: vec2<f32>,
    @location(2) @interpolate(flat) radii_x: vec4<f32>,
    @location(3) @interpolate(flat) radii_y: vec4<f32>,
    @location(4) @interpolate(flat) background: vec4<f32>,
    @location(5) @interpolate(flat) border_top_color: vec4<f32>,
    @location(6) @interpolate(flat) border_right_color: vec4<f32>,
    @location(7) @interpolate(flat) border_bottom_color: vec4<f32>,
    @location(8) @interpolate(flat) border_left_color: vec4<f32>,
    @location(9) @interpolate(flat) outline_color: vec4<f32>,
    @location(10) @interpolate(flat) border_widths: vec4<f32>,
    @location(11) @interpolate(flat) border_styles: vec4<u32>,
    @location(12) @interpolate(flat) outline_width: f32,
    @location(13) @interpolate(flat) outline_offset: f32,
    @location(14) @interpolate(flat) clip_range: vec2<u32>,
}

@vertex
fn vertex_main(instance: Instance, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
        vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    );
    let corner = corners[vertex_index];
    let expansion = max(0.0, instance.outline_width + instance.outline_offset) + 1.5;
    let local = corner * (instance.half_size + vec2(expansion));
    let pixel = instance.center + local;
    let clip = vec2(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
    );

    var output: VertexOutput;
    output.clip_position = vec4(clip, 0.0, 1.0);
    output.local_position = local;
    output.half_size = instance.half_size;
    output.radii_x = instance.radii_x;
    output.radii_y = instance.radii_y;
    output.background = instance.background;
    output.border_top_color = instance.border_top_color;
    output.border_right_color = instance.border_right_color;
    output.border_bottom_color = instance.border_bottom_color;
    output.border_left_color = instance.border_left_color;
    output.outline_color = instance.outline_color;
    output.border_widths = instance.border_widths;
    output.border_styles = instance.border_styles;
    output.outline_width = instance.outline_width;
    output.outline_offset = instance.outline_offset;
    output.clip_range = instance.clip_range;
    return output;
}

fn corner_radius(position: vec2<f32>, radii_x: vec4<f32>, radii_y: vec4<f32>) -> vec2<f32> {
    if position.y < 0.0 {
        return select(vec2(radii_x.x, radii_y.x), vec2(radii_x.y, radii_y.y), position.x >= 0.0);
    }
    return select(vec2(radii_x.w, radii_y.w), vec2(radii_x.z, radii_y.z), position.x >= 0.0);
}

fn rounded_box_distance(
    position: vec2<f32>,
    half_size: vec2<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
) -> f32 {
    let radius = corner_radius(position, radii_x, radii_y);
    let rectangle_distance = abs(position) - half_size;
    if radius.x <= 0.0 || radius.y <= 0.0 {
        return min(max(rectangle_distance.x, rectangle_distance.y), 0.0)
            + length(max(rectangle_distance, vec2(0.0)));
    }
    let q = rectangle_distance + radius;
    if q.x > 0.0 && q.y > 0.0 {
        return (length(q / radius) - 1.0) * min(radius.x, radius.y);
    }
    return max(rectangle_distance.x, rectangle_distance.y);
}

fn border_side_index(position: vec2<f32>, half_size: vec2<f32>, widths: vec4<f32>) -> u32 {
    let distances = vec4(
        position.y + half_size.y,
        half_size.x - position.x,
        half_size.y - position.y,
        position.x + half_size.x,
    );
    let ratios = select(vec4(1e20), distances / max(widths, vec4(0.0001)), widths > vec4(0.0));
    var side = 0u;
    var closest = ratios.x;
    if ratios.y < closest {
        side = 1u;
        closest = ratios.y;
    }
    if ratios.z < closest {
        side = 2u;
        closest = ratios.z;
    }
    if ratios.w < closest {
        side = 3u;
    }
    return side;
}

fn side_color(input: VertexOutput, side: u32) -> vec4<f32> {
    if side == 0u {
        return input.border_top_color;
    }
    if side == 1u {
        return input.border_right_color;
    }
    if side == 2u {
        return input.border_bottom_color;
    }
    return input.border_left_color;
}

fn side_width(widths: vec4<f32>, side: u32) -> f32 {
    if side == 0u {
        return widths.x;
    }
    if side == 1u {
        return widths.y;
    }
    if side == 2u {
        return widths.z;
    }
    return widths.w;
}

fn side_style(styles: vec4<u32>, side: u32) -> u32 {
    if side == 0u {
        return styles.x;
    }
    if side == 1u {
        return styles.y;
    }
    if side == 2u {
        return styles.z;
    }
    return styles.w;
}

fn side_depth(position: vec2<f32>, half_size: vec2<f32>, side: u32) -> f32 {
    if side == 0u {
        return position.y + half_size.y;
    }
    if side == 1u {
        return half_size.x - position.x;
    }
    if side == 2u {
        return half_size.y - position.y;
    }
    return position.x + half_size.x;
}

fn border_pattern(style: u32, side: u32, position: vec2<f32>, width: f32, depth: f32) -> f32 {
    if style <= 1u {
        return 0.0;
    }
    let along = select(position.y, position.x, side == 0u || side == 2u);
    if style == 2u {
        return select(0.0, 1.0, fract(along / max(width * 2.0, 1.0)) < 0.5);
    }
    if style == 3u {
        return select(0.0, 1.0, fract(along / max(width * 5.0, 1.0)) < 0.6);
    }
    if style == 5u {
        let fraction = depth / max(width, 0.0001);
        return select(0.0, 1.0, fraction <= 0.333 || fraction >= 0.667);
    }
    return 1.0;
}

fn styled_border_color(color: vec4<f32>, style: u32, side: u32, depth: f32, width: f32) -> vec4<f32> {
    var shade = 1.0;
    let upper_or_left = side == 0u || side == 3u;
    if style == 8u {
        shade = select(1.25, 0.65, upper_or_left);
    } else if style == 9u {
        shade = select(0.65, 1.25, upper_or_left);
    } else if style == 6u || style == 7u {
        let outer_half = depth < width * 0.5;
        let dark = select(upper_or_left == outer_half, upper_or_left != outer_half, style == 7u);
        shade = select(1.25, 0.65, dark);
    }
    return vec4(clamp(color.rgb * shade, vec3(0.0), vec3(1.0)), color.a);
}

fn srgb_channel_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    return pow((value + 0.055) / 1.055, 2.4);
}

fn linear_color(color: vec4<f32>) -> vec4<f32> {
    return vec4(
        srgb_channel_to_linear(color.r),
        srgb_channel_to_linear(color.g),
        srgb_channel_to_linear(color.b),
        color.a,
    );
}

fn composite_over(foreground: vec4<f32>, background: vec4<f32>) -> vec4<f32> {
    let alpha = foreground.a + background.a * (1.0 - foreground.a);
    if alpha <= 0.0001 {
        return vec4(0.0);
    }
    let rgb = (
        foreground.rgb * foreground.a
        + background.rgb * background.a * (1.0 - foreground.a)
    ) / alpha;
    return vec4(rgb, alpha);
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = input.clip_position.xy;
    var clip_coverage = 1.0;
    for (var index = 0u; index < input.clip_range.y; index += 1u) {
        let clip = clips[input.clip_range.x + index];
        let distance = rounded_box_distance(
            pixel - clip.center,
            clip.half_size,
            clip.radii_x,
            clip.radii_y,
        );
        let antialias = max(fwidth(distance), 0.75);
        clip_coverage = min(clip_coverage, 1.0 - smoothstep(-antialias, antialias, distance));
    }
    if clip_coverage <= 0.0001 {
        discard;
    }

    let border_widths = max(input.border_widths, vec4(0.0));
    let outline_width = max(input.outline_width, 0.0);
    let outline_offset = max(input.outline_offset, 0.0);
    let base_distance = rounded_box_distance(
        input.local_position,
        input.half_size,
        input.radii_x,
        input.radii_y,
    );
    let antialias = max(fwidth(base_distance), 0.75);
    let base_coverage = 1.0 - smoothstep(-antialias, antialias, base_distance);

    let top_width = border_widths.x;
    let right_width = border_widths.y;
    let bottom_width = border_widths.z;
    let left_width = border_widths.w;
    let inner_center = vec2(
        (left_width - right_width) * 0.5,
        (top_width - bottom_width) * 0.5,
    );
    let inner_half_size = max(
        input.half_size - vec2(
            (left_width + right_width) * 0.5,
            (top_width + bottom_width) * 0.5,
        ),
        vec2(0.0),
    );
    let inner_radii_x = max(
        input.radii_x - vec4(left_width, right_width, right_width, left_width),
        vec4(0.0),
    );
    let inner_radii_y = max(
        input.radii_y - vec4(top_width, top_width, bottom_width, bottom_width),
        vec4(0.0),
    );
    let inner_distance = rounded_box_distance(
        input.local_position - inner_center,
        inner_half_size,
        inner_radii_x,
        inner_radii_y,
    );
    let inner_coverage = 1.0 - smoothstep(-antialias, antialias, inner_distance);
    var base_color = input.background;
    if any(border_widths > vec4(0.0)) {
        let side = border_side_index(input.local_position, input.half_size, border_widths);
        let width = side_width(border_widths, side);
        let depth = side_depth(input.local_position, input.half_size, side);
        let style = side_style(input.border_styles, side);
        let pattern = border_pattern(style, side, input.local_position, width, depth);
        var color = styled_border_color(side_color(input, side), style, side, depth, width);
        color.a *= (1.0 - inner_coverage) * pattern;
        base_color = composite_over(color, input.background);
    }
    base_color.a *= base_coverage;

    let outline_inner_expansion = outline_offset;
    let outline_outer_expansion = outline_offset + outline_width;
    let outline_inner_distance = rounded_box_distance(
        input.local_position,
        input.half_size + vec2(outline_inner_expansion),
        input.radii_x + vec4(outline_inner_expansion),
        input.radii_y + vec4(outline_inner_expansion),
    );
    let outline_outer_distance = rounded_box_distance(
        input.local_position,
        input.half_size + vec2(outline_outer_expansion),
        input.radii_x + vec4(outline_outer_expansion),
        input.radii_y + vec4(outline_outer_expansion),
    );
    let outline_inner_coverage = 1.0 - smoothstep(-antialias, antialias, outline_inner_distance);
    let outline_outer_coverage = 1.0 - smoothstep(-antialias, antialias, outline_outer_distance);
    let outline_coverage = outline_outer_coverage * (1.0 - outline_inner_coverage);
    var outline_color = input.outline_color;
    outline_color.a *= outline_coverage;

    let unclipped_alpha = base_color.a + outline_color.a * (1.0 - base_color.a);
    let combined_alpha = unclipped_alpha * clip_coverage;
    if combined_alpha <= 0.0001 {
        discard;
    }
    let combined_rgb = (
        base_color.rgb * base_color.a
        + outline_color.rgb * outline_color.a * (1.0 - base_color.a)
    ) / unclipped_alpha;
    return linear_color(vec4(combined_rgb, combined_alpha));
}
