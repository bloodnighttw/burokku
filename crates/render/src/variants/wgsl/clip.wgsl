struct ScreenUniform {
    size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> screen: ScreenUniform;

struct VertexInput {
    @location(0) bounds: vec4<f32>,
    @location(1) corners: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local_position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) corners: vec4<f32>,
};

@vertex
fn vertex_main(
    input: VertexInput,
    @builtin(vertex_index) vertex_index: u32,
) -> VertexOutput {
    let vertices = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    let local = vertices[vertex_index] * input.bounds.zw;
    let pixel = input.bounds.xy + local;
    let clip = vec2<f32>(
        pixel.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.size.y * 2.0,
    );
    return VertexOutput(
        vec4<f32>(clip, 0.0, 1.0),
        local,
        input.bounds.zw,
        input.corners,
    );
}

fn rounded_distance(position: vec2<f32>, size: vec2<f32>, corners: vec4<f32>) -> f32 {
    let centered = position - size * 0.5;
    let top_radius = select(corners.x, corners.y, centered.x > 0.0);
    let bottom_radius = select(corners.w, corners.z, centered.x > 0.0);
    let radius = select(top_radius, bottom_radius, centered.y > 0.0);
    let corner = abs(centered) - size * 0.5 + vec2<f32>(radius);
    return min(max(corner.x, corner.y), 0.0)
        + length(max(corner, vec2<f32>(0.0))) - radius;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if rounded_distance(input.local_position, input.size, input.corners) > 0.0 {
        discard;
    }
    return vec4<f32>(0.0);
}
