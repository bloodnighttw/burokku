struct Screen {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> screen: Screen;

struct Clip {
    center: vec2<f32>,
    half_size: vec2<f32>,
    radii: vec4<f32>,
}

@group(0) @binding(1)
var<storage, read> clips: array<Clip>;

@group(0) @binding(2)
var background_images: texture_2d_array<f32>;

@group(0) @binding(3)
var background_image_sampler: sampler;

struct Instance {
    @location(0) center: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) radii: vec4<f32>,
    @location(3) background: vec4<f32>,
    @location(4) border_color: vec4<f32>,
    @location(5) outline_color: vec4<f32>,
    @location(6) border_width: f32,
    @location(7) outline_width: f32,
    @location(8) outline_offset: f32,
    @location(9) _padding: f32,
    @location(10) clip_range: vec2<u32>,
    @location(11) gradient_color: vec4<f32>,
    @location(12) gradient: vec4<f32>,
    @location(13) transform_x: vec3<f32>,
    @location(14) transform_y: vec3<f32>,
    @location(15) image: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_position: vec2<f32>,
    @location(1) @interpolate(flat) half_size: vec2<f32>,
    @location(2) @interpolate(flat) radii: vec4<f32>,
    @location(3) @interpolate(flat) background: vec4<f32>,
    @location(4) @interpolate(flat) border_color: vec4<f32>,
    @location(5) @interpolate(flat) outline_color: vec4<f32>,
    @location(6) @interpolate(flat) widths: vec3<f32>,
    @location(7) @interpolate(flat) clip_range: vec2<u32>,
    @location(8) @interpolate(flat) gradient_color: vec4<f32>,
    @location(9) @interpolate(flat) gradient: vec4<f32>,
    @location(10) @interpolate(flat) effect_blur: f32,
    @location(11) @interpolate(flat) image: vec4<f32>,
}

@vertex
fn vertex_main(instance: Instance, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
        vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    );
    let corner = corners[vertex_index];
    let expansion = max(0.0, instance.outline_width + instance.outline_offset) + instance._padding * 2.0 + 1.5;
    let local = corner * (instance.half_size + vec2(expansion));
    let pixel = instance.center + vec2(
        dot(instance.transform_x.xy, local) + instance.transform_x.z,
        dot(instance.transform_y.xy, local) + instance.transform_y.z,
    );
    let clip = vec2(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
    );

    var output: VertexOutput;
    output.clip_position = vec4(clip, 0.0, 1.0);
    output.local_position = local;
    output.half_size = instance.half_size;
    output.radii = instance.radii;
    output.background = instance.background;
    output.border_color = instance.border_color;
    output.outline_color = instance.outline_color;
    output.widths = vec3(instance.border_width, instance.outline_width, instance.outline_offset);
    output.clip_range = instance.clip_range;
    output.gradient_color = instance.gradient_color;
    output.gradient = instance.gradient;
    output.effect_blur = instance._padding;
    output.image = instance.image;
    return output;
}

fn corner_radius(position: vec2<f32>, radii: vec4<f32>) -> f32 {
    if position.y < 0.0 {
        return select(radii.x, radii.y, position.x >= 0.0);
    }
    return select(radii.w, radii.z, position.x >= 0.0);
}

fn rounded_box_distance(position: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let radius = corner_radius(position, radii);
    let q = abs(position) - half_size + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
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

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = input.clip_position.xy;
    var clip_coverage = 1.0;
    for (var index = 0u; index < input.clip_range.y; index += 1u) {
        let clip = clips[input.clip_range.x + index];
        let distance = rounded_box_distance(pixel - clip.center, clip.half_size, clip.radii);
        let antialias = max(fwidth(distance), 0.75);
        clip_coverage = min(
            clip_coverage,
            1.0 - smoothstep(-antialias, antialias, distance),
        );
    }
    if clip_coverage <= 0.0001 {
        discard;
    }

    let border_width = max(input.widths.x, 0.0);
    let outline_width = max(input.widths.y, 0.0);
    let outline_offset = max(input.widths.z, 0.0);
    let base_distance = rounded_box_distance(input.local_position, input.half_size, input.radii);
    let antialias = max(fwidth(base_distance), 0.75);
    let base_coverage = 1.0 - smoothstep(
        -antialias - input.effect_blur,
        antialias + input.effect_blur,
        base_distance,
    );

    let inner_half_size = max(input.half_size - vec2(border_width), vec2(0.0));
    let inner_radii = max(input.radii - vec4(border_width), vec4(0.0));
    let inner_distance = rounded_box_distance(input.local_position, inner_half_size, inner_radii);
    let inner_coverage = 1.0 - smoothstep(-antialias, antialias, inner_distance);
    var base_color = input.background;
    if input.gradient.z == 1.0 {
        let normalized = input.local_position / max(input.half_size, vec2(0.0001));
        let gradient_position = clamp(dot(normalized, input.gradient.xy) * 0.5 + 0.5, 0.0, 1.0);
        base_color = mix(input.background, input.gradient_color, gradient_position);
    } else if input.gradient.z == 2.0 {
        let gradient_position = clamp(length(input.local_position / max(input.half_size, vec2(0.0001))), 0.0, 1.0);
        base_color = mix(input.background, input.gradient_color, gradient_position);
    } else if input.gradient.z == 3.0 {
        let box_uv = clamp(input.local_position / max(input.half_size, vec2(0.0001)) * 0.5 + 0.5, vec2(0.0), vec2(1.0));
        var image_color = textureSample(
            background_images,
            background_image_sampler,
            box_uv * input.image.xy,
            i32(input.image.z),
        );
        image_color.a *= input.image.w;
        let combined_alpha = image_color.a + base_color.a * (1.0 - image_color.a);
        if combined_alpha > 0.0001 {
            let combined_rgb = (
                image_color.rgb * image_color.a
                + base_color.rgb * base_color.a * (1.0 - image_color.a)
            ) / combined_alpha;
            base_color = vec4(combined_rgb, combined_alpha);
        }
    }
    if border_width > 0.0 {
        base_color = mix(input.border_color, base_color, inner_coverage);
    }
    base_color.a *= base_coverage;

    let outline_inner_expansion = outline_offset;
    let outline_outer_expansion = outline_offset + outline_width;
    let outline_inner_distance = rounded_box_distance(
        input.local_position,
        input.half_size + vec2(outline_inner_expansion),
        input.radii + vec4(outline_inner_expansion),
    );
    let outline_outer_distance = rounded_box_distance(
        input.local_position,
        input.half_size + vec2(outline_outer_expansion),
        input.radii + vec4(outline_outer_expansion),
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
    let combined_rgb = (base_color.rgb * base_color.a + outline_color.rgb * outline_color.a * (1.0 - base_color.a)) / unclipped_alpha;
    return linear_color(vec4(combined_rgb, combined_alpha));
}
