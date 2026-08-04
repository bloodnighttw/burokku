struct Composite {
    destination: vec4<f32>,
    source: vec4<f32>,
    effect: vec4<f32>,
    transform_x: vec4<f32>,
    transform_y: vec4<f32>,
}

@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

@group(0) @binding(2)
var<uniform> composite: Composite;

struct Clip {
    center: vec2<f32>,
    half_size: vec2<f32>,
    radii: vec4<f32>,
    inverse_x: vec3<f32>,
    inverse_y: vec3<f32>,
}

@group(0) @binding(3)
var<storage, read> clips: array<Clip>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let uv = corners[vertex_index];
    let pixel = composite.source.xy + uv * composite.source.zw;
    let relative = pixel - composite.effect.xy;
    let transformed = composite.effect.xy + vec2(
        dot(composite.transform_x.xy, relative) + composite.transform_x.z,
        dot(composite.transform_y.xy, relative) + composite.transform_y.z,
    );
    let target_pixel = transformed - composite.destination.zw;
    var output: VertexOutput;
    output.position = vec4(
        target_pixel.x / composite.destination.x * 2.0 - 1.0,
        1.0 - target_pixel.y / composite.destination.y * 2.0,
        0.0,
        1.0,
    );
    output.uv = uv;
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

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var clip_coverage = 1.0;
    let pixel = input.position.xy + composite.destination.zw;
    for (var index = 0u; index < u32(composite.effect.w); index += 1u) {
        let clip = clips[index];
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

    var color = textureSample(source_texture, source_sampler, input.uv);
    if color.a > 0.0001 {
        color = vec4(color.rgb / color.a, color.a);
    }
    color.a *= composite.effect.z * clip_coverage;
    return color;
}
