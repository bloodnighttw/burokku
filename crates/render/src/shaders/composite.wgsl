struct Composite {
    screen_origin: vec4<f32>,
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
    let pixel = uv * composite.screen_origin.xy;
    let relative = pixel - composite.screen_origin.zw;
    let transformed = composite.screen_origin.zw + vec2(
        dot(composite.transform_x.xy, relative) + composite.transform_x.z,
        dot(composite.transform_y.xy, relative) + composite.transform_y.z,
    );
    var output: VertexOutput;
    output.position = vec4(
        transformed.x / composite.screen_origin.x * 2.0 - 1.0,
        1.0 - transformed.y / composite.screen_origin.y * 2.0,
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
    for (var index = 0u; index < u32(composite.transform_y.w); index += 1u) {
        let clip = clips[index];
        let relative = input.position.xy - clip.center;
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
        color.rgb /= color.a;
    }
    color.a *= composite.transform_x.w * clip_coverage;
    return color;
}
