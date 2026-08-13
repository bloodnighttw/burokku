// Copyright 2026 Burokku contributors
// SPDX-License-Identifier: MIT
//
// WGSL adaptation informed by the MIT-licensed optical model in
// https://github.com/whynotmake-it/flutter_liquid_glass

struct FrameUniform {
    // xy: render-target size, z: elapsed seconds
    resolution_time: vec4<f32>,
    // xy: animated light direction, z: instance count
    light_and_count: vec4<f32>,
};

struct VertexInput {
    // xy: center in pixels, zw: size in pixels
    @location(0) rect: vec4<f32>,
    // radius, thickness, refractive index, chromatic aberration
    @location(1) optics: vec4<f32>,
    @location(2) tint: vec4<f32>,
    // phase, wobble amplitude, light variation, opacity
    @location(3) motion: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) pixel_position: vec2<f32>,
    @location(1) local_position: vec2<f32>,
    @location(2) @interpolate(flat) half_size: vec2<f32>,
    @location(3) @interpolate(flat) optics: vec4<f32>,
    @location(4) @interpolate(flat) tint: vec4<f32>,
    @location(5) @interpolate(flat) motion: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniform;
@group(0) @binding(1) var backdrop: texture_2d<f32>;
@group(0) @binding(2) var backdrop_sampler: sampler;

@vertex
fn vertex_main(input: VertexInput, @builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let corner = corners[vertex_index];
    let half_size = input.rect.zw * 0.5;
    let wobble = vec2<f32>(
        sin(frame.resolution_time.z * 0.73 + input.motion.x),
        cos(frame.resolution_time.z * 0.61 + input.motion.x * 1.17),
    ) * input.motion.y;
    let center = input.rect.xy + wobble;
    let local_position = corner * half_size;
    let pixel_position = center + local_position;
    let normalized = pixel_position / frame.resolution_time.xy;
    let clip = normalized * 2.0 - vec2<f32>(1.0, 1.0);

    var output: VertexOutput;
    output.clip_position = vec4<f32>(clip.x, -clip.y, 0.0, 1.0);
    output.pixel_position = pixel_position;
    output.local_position = local_position;
    output.half_size = half_size;
    output.optics = input.optics;
    output.tint = input.tint;
    output.motion = input.motion;
    return output;
}

fn rounded_rect_sdf(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let safe_radius = min(radius, min(half_size.x, half_size.y));
    let q = abs(point) - (half_size - vec2<f32>(safe_radius));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - safe_radius;
}

fn liquid_height(signed_distance: f32, thickness: f32) -> f32 {
    if signed_distance >= 0.0 || thickness <= 0.0 {
        return 0.0;
    }
    if signed_distance < -thickness {
        return thickness;
    }
    let x = thickness + signed_distance;
    return sqrt(max(0.0, thickness * thickness - x * x));
}

fn surface_normal(signed_distance: f32, thickness: f32) -> vec3<f32> {
    let derivative = vec2<f32>(dpdx(signed_distance), dpdy(signed_distance));
    var gradient = vec2<f32>(0.0, -1.0);
    let derivative_length = length(derivative);
    if derivative_length > 0.0001 {
        gradient = derivative / derivative_length;
    }

    let edge_slope = clamp((thickness + signed_distance) / max(thickness, 0.001), 0.0, 1.0);
    let facing = sqrt(max(0.0, 1.0 - edge_slope * edge_slope));
    return normalize(vec3<f32>(gradient * edge_slope, facing));
}

fn refraction_offset(
    normal: vec3<f32>,
    height: f32,
    thickness: f32,
    refractive_index: f32,
) -> vec2<f32> {
    let incident = vec3<f32>(0.0, 0.0, -1.0);
    let ray = refract(incident, normal, 1.0 / max(refractive_index, 1.001));
    let travel = height + thickness * 5.0;
    return ray.xy * travel / max(abs(ray.z), 0.08);
}

fn backdrop_at(uv: vec2<f32>) -> vec3<f32> {
    return textureSample(
        backdrop,
        backdrop_sampler,
        clamp(uv, vec2<f32>(0.001), vec2<f32>(0.999)),
    ).rgb;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let radius = input.optics.x;
    let thickness = input.optics.y;
    let refractive_index = input.optics.z;
    let chromatic_aberration = input.optics.w;
    let signed_distance = rounded_rect_sdf(input.local_position, input.half_size, radius);
    let coverage = 1.0 - smoothstep(-1.15, 1.15, signed_distance);
    if coverage < 0.002 {
        discard;
    }

    let normal = surface_normal(signed_distance, thickness);
    let height = liquid_height(signed_distance, thickness);
    let displacement_px = refraction_offset(normal, height, thickness, refractive_index);
    let inverse_size = 1.0 / frame.resolution_time.xy;
    let uv = input.pixel_position * inverse_size;
    let displacement_uv = displacement_px * inverse_size;

    // Slightly different refraction per color channel creates spectral fringes.
    let spread = 0.055 * chromatic_aberration;
    let red = backdrop_at(uv + displacement_uv * (1.0 + spread)).r;
    let green = backdrop_at(uv + displacement_uv).g;
    let blue = backdrop_at(uv + displacement_uv * (1.0 - spread)).b;
    var glass = vec3<f32>(red, green, blue);

    // A cheap cross filter gives the pane a subtle frosted body without another pass.
    let frost_radius = (0.75 + chromatic_aberration) * inverse_size;
    let frost = (
        backdrop_at(uv + displacement_uv + vec2<f32>( frost_radius.x, 0.0)) +
        backdrop_at(uv + displacement_uv + vec2<f32>(-frost_radius.x, 0.0)) +
        backdrop_at(uv + displacement_uv + vec2<f32>(0.0,  frost_radius.y)) +
        backdrop_at(uv + displacement_uv + vec2<f32>(0.0, -frost_radius.y))
    ) * 0.25;
    glass = mix(glass, frost, 0.16);

    let luminance = dot(glass, vec3<f32>(0.299, 0.587, 0.114));
    glass = mix(vec3<f32>(luminance), glass, 1.12);
    glass = mix(glass, input.tint.rgb, input.tint.a);

    let rim_x = signed_distance / 1.7;
    let rim = 1.0 / (1.0 + 0.89 * rim_x * rim_x);
    let light_direction = normalize(frame.light_and_count.xy);
    let forward_light = max(dot(normal.xy, light_direction), 0.0);
    let reverse_light = max(dot(normal.xy, -light_direction), 0.0) * 0.55;
    let directional = (forward_light * forward_light + reverse_light * reverse_light) * 0.72;
    let fresnel = pow(clamp(1.0 - normal.z, 0.0, 1.0), 3.0);
    let highlight = mix(vec3<f32>(1.0), input.tint.rgb, 0.22);
    glass += highlight * rim * (0.10 + directional + fresnel * 0.35);

    let opposite = max(dot(normal.xy, -light_direction), 0.0);
    glass -= vec3<f32>(opposite * rim * 0.07);

    return vec4<f32>(clamp(glass, vec3<f32>(0.0), vec3<f32>(1.0)), coverage * input.motion.w);
}
