struct Screen {
    size: vec2<f32>,
    origin: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> screen: Screen;

struct Clip {
    center: vec2<f32>,
    half_size: vec2<f32>,
    radii: vec4<f32>,
    inverse_x: vec3<f32>,
    inverse_y: vec3<f32>,
}

@group(0) @binding(1)
var<storage, read> clips: array<Clip>;

@group(0) @binding(2)
var background_images: texture_2d_array<f32>;

@group(0) @binding(3)
var background_image_sampler: sampler;

struct GradientStop {
    color: vec4<f32>,
    position: f32,
}

@group(0) @binding(4)
var<storage, read> gradient_stops: array<GradientStop>;

struct InsetShadow {
    geometry: vec4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(5)
var<storage, read> inset_shadows: array<InsetShadow>;

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
    @location(11) gradient: vec4<f32>,
    @location(12) transform_x: vec3<f32>,
    @location(13) transform_y: vec3<f32>,
    @location(14) image: vec4<f32>,
    @location(15) inset_shadow_range: vec2<u32>,
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
    @location(8) @interpolate(flat) gradient: vec4<f32>,
    @location(9) @interpolate(flat) effect_blur: f32,
    @location(10) @interpolate(flat) image: vec4<f32>,
    @location(11) @interpolate(flat) inset_shadow_range: vec2<u32>,
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
    let target_pixel = pixel - screen.origin;
    let clip = vec2(
        target_pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - target_pixel.y / screen.size.y * 2.0,
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
    output.gradient = instance.gradient;
    output.effect_blur = instance._padding;
    output.image = instance.image;
    output.inset_shadow_range = instance.inset_shadow_range;
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

fn composite(over: vec4<f32>, under: vec4<f32>) -> vec4<f32> {
    let alpha = over.a + under.a * (1.0 - over.a);
    if alpha <= 0.0001 {
        return vec4(0.0);
    }
    return vec4(
        (over.rgb * over.a + under.rgb * under.a * (1.0 - over.a)) / alpha,
        alpha,
    );
}

fn sample_gradient(position: f32, range: vec2<f32>) -> vec4<f32> {
    let start = u32(range.x);
    let count = u32(range.y);
    if count == 0u {
        return vec4(0.0);
    }
    var previous = gradient_stops[start];
    if position <= previous.position || count == 1u {
        return previous.color;
    }
    for (var index = 1u; index < count; index += 1u) {
        let current = gradient_stops[start + index];
        if position <= current.position {
            let span = max(current.position - previous.position, 0.0001);
            return mix(previous.color, current.color, clamp((position - previous.position) / span, 0.0, 1.0));
        }
        previous = current;
    }
    return previous.color;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = input.clip_position.xy + screen.origin;
    var clip_coverage = 1.0;
    for (var index = 0u; index < input.clip_range.y; index += 1u) {
        let clip = clips[input.clip_range.x + index];
        let relative = pixel - clip.center;
        let local = vec2(
            dot(clip.inverse_x.xy, relative) + clip.inverse_x.z,
            dot(clip.inverse_y.xy, relative) + clip.inverse_y.z,
        );
        let distance = rounded_box_distance(local, clip.half_size, clip.radii);
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
        base_color = composite(sample_gradient(gradient_position, input.image.xy), base_color);
    } else if input.gradient.z == 2.0 {
        let gradient_position = clamp(length(input.local_position / max(input.half_size, vec2(0.0001))), 0.0, 1.0);
        base_color = composite(sample_gradient(gradient_position, input.image.xy), base_color);
    } else if input.gradient.z == 3.0 {
        let box_uv = clamp(input.local_position / max(input.half_size, vec2(0.0001)) * 0.5 + 0.5, vec2(0.0), vec2(1.0));
        var image_color = textureSample(
            background_images,
            background_image_sampler,
            box_uv * input.image.xy,
            i32(input.image.z),
        );
        image_color.a *= input.image.w;
        base_color = composite(image_color, base_color);
    }
    for (var shadow_index = 0u; shadow_index < input.inset_shadow_range.y; shadow_index += 1u) {
        let shadow = inset_shadows[input.inset_shadow_range.x + shadow_index];
        let shifted = input.local_position - shadow.geometry.xy;
        let width = max(shadow.geometry.z * 1.5 + shadow.geometry.w, 0.5);
        let inset_half_size = max(input.half_size - vec2(width), vec2(0.0));
        let inset_radii = max(input.radii - vec4(width), vec4(0.0));
        let inset_distance = rounded_box_distance(shifted, inset_half_size, inset_radii);
        let inset_antialias = max(fwidth(inset_distance), max(shadow.geometry.z, 0.75));
        let inner_coverage = 1.0 - smoothstep(-inset_antialias, inset_antialias, inset_distance);
        var shadow_color = shadow.color;
        shadow_color.a *= (1.0 - inner_coverage) * base_coverage;
        base_color = composite(shadow_color, base_color);
    }
    if border_width > 0.0 {
        base_color = mix(input.border_color, base_color, inner_coverage);
    }
    base_color.a *= base_coverage * input.gradient.w;

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
    outline_color.a *= outline_coverage * input.gradient.w;

    let unclipped_alpha = base_color.a + outline_color.a * (1.0 - base_color.a);
    let combined_alpha = unclipped_alpha * clip_coverage;
    if combined_alpha <= 0.0001 {
        discard;
    }
    let combined_rgb = (base_color.rgb * base_color.a + outline_color.rgb * outline_color.a * (1.0 - base_color.a)) / unclipped_alpha;
    return linear_color(vec4(combined_rgb, combined_alpha));
}
