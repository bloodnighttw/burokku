struct ScreenUniform {
    size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: ScreenUniform;

struct ClipMask {
    bounds: vec4<f32>,
    round: vec4<f32>,
};

@group(0) @binding(1)
var<storage, read> clip_masks: array<ClipMask>;

struct VertexInput {
    @location(0) bounds: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) round: vec4<f32>,
    @location(3) basis: vec4<f32>,
    @location(4) clip_range: vec2<u32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) round: vec4<f32>,
    @location(4) pixel_position: vec2<f32>,
    @location(5) @interpolate(flat) clip_range: vec2<u32>,
};

@vertex
fn vertex_main(
    input: VertexInput,
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let local = corners[vertex_index] * input.bounds.zw;
    let pixel = input.bounds.xy
        + input.basis.xy * local.x
        + input.basis.zw * local.y;
    let clip = vec2<f32>(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
    );
    return VertexOutput(
        vec4<f32>(clip, 0.0, 1.0),
        input.color,
        local,
        input.bounds.zw,
        input.round,
        pixel,
        input.clip_range,
    );
}

fn rounded_distance(position: vec2<f32>, bounds: vec4<f32>, round: vec4<f32>) -> f32 {
    let centered = position - bounds.xy - bounds.zw * 0.5;
    let top_radius = select(round.x, round.y, centered.x > 0.0);
    let bottom_radius = select(round.w, round.z, centered.x > 0.0);
    let radius = select(top_radius, bottom_radius, centered.y > 0.0);
    let corner = abs(centered) - bounds.zw * 0.5 + vec2<f32>(radius);
    return min(max(corner.x, corner.y), 0.0)
        + length(max(corner, vec2<f32>(0.0))) - radius;
}

fn coverage(distance: f32) -> f32 {
    return clamp(0.5 - distance, 0.0, 1.0);
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var mask = coverage(rounded_distance(
        input.local_position,
        vec4<f32>(0.0, 0.0, input.size),
        input.round,
    ));
    for (var index = 0u; index < input.clip_range.y; index += 1u) {
        let clip_mask = clip_masks[input.clip_range.x + index];
        mask *= coverage(rounded_distance(
            input.pixel_position,
            clip_mask.bounds,
            clip_mask.round,
        ));
    }

    return vec4<f32>(input.color.rgb, input.color.a * mask);
}
