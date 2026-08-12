use std::{env, fs::File, io, path::PathBuf};

use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use render::{
    canvas::DrawList,
    offscreen::OffscreenSurface,
    shapes::{
        rect::{DrawRectExt, Rect},
        round::Round,
        stroke::{DrawStrokeExt, Stroke},
    },
    wgpu,
    wgsl::WgslBackdrop,
};

const SIZE: [u32; 2] = [800, 500];

/*
The shader below adapts the rendering approach from
https://github.com/whynotmake-it/flutter_liquid_glass:
rounded-rectangle SDF geometry, a curved surface normal, refracted backdrop
sampling, chromatic dispersion, glass tint, saturation, and rim lighting.

Copyright 2025 Tim Lehmann for whynotmake.it

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

The upstream implementation receives an already-blurred backdrop. This
self-contained example approximates that stage with a five-tap frost sample.
*/
const LIQUID_GLASS_WGSL: &str = r#"
const LIQUID_LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

fn liquid_rounded_rect_sdf(
    position: vec2<f32>,
    size: vec2<f32>,
    radius: f32,
) -> f32 {
    let half_size = size * 0.5;
    let centered = position - half_size;
    let corner = abs(centered) - half_size + vec2<f32>(radius);
    return min(max(corner.x, corner.y), 0.0)
        + length(max(corner, vec2<f32>(0.0))) - radius;
}

fn liquid_surface_height(distance: f32, thickness: f32) -> f32 {
    if distance >= 0.0 {
        return 0.0;
    }
    if distance < -thickness {
        return thickness;
    }
    let edge_depth = thickness + distance;
    return sqrt(max(0.0, thickness * thickness - edge_depth * edge_depth));
}

fn liquid_surface_normal(distance: f32, thickness: f32) -> vec3<f32> {
    let gradient = vec2<f32>(dpdx(distance), dpdy(distance));
    let normal_xy = clamp((thickness + distance) / thickness, 0.0, 1.0);
    let normal_z = sqrt(max(0.0, 1.0 - normal_xy * normal_xy));
    return normalize(vec3<f32>(gradient * normal_xy, normal_z));
}

fn liquid_frost_sample(
    uv: vec2<f32>,
    texel_size: vec2<f32>,
    blur_radius: f32,
) -> vec4<f32> {
    let spread = texel_size * blur_radius;
    let horizontal = vec2<f32>(spread.x, 0.0);
    let vertical = vec2<f32>(0.0, spread.y);
    return sample_backdrop(uv) * 0.4
        + (sample_backdrop(uv + horizontal) + sample_backdrop(uv - horizontal)
            + sample_backdrop(uv + vertical) + sample_backdrop(uv - vertical)) * 0.15;
}

fn liquid_apply_tint(color: vec3<f32>, tint: vec4<f32>) -> vec3<f32> {
    let tint_luminance = dot(tint.rgb, LIQUID_LUMA);
    var tinted: vec3<f32>;
    if tint_luminance < 0.5 {
        tinted = color * tint.rgb * 2.0;
    } else {
        tinted = vec3<f32>(1.0)
            - (vec3<f32>(1.0) - color) * (vec3<f32>(1.0) - tint.rgb);
    }
    return mix(color, tinted, clamp(tint.a, 0.0, 1.0));
}

fn liquid_highlight_color(background: vec3<f32>) -> vec3<f32> {
    let luminance = dot(background, LIQUID_LUMA);
    let largest = max(max(background.r, background.g), background.b);
    let smallest = min(min(background.r, background.g), background.b);
    let saturation = (largest - smallest) / max(largest, 0.001);
    let normalized = background / max(luminance, 0.001);
    let gray = vec3<f32>(dot(normalized, LIQUID_LUMA));
    let colored = clamp(mix(gray, normalized, 1.3), vec3<f32>(0.0), vec3<f32>(1.0));
    let color_influence = smoothstep(0.0, 0.6, luminance)
        * smoothstep(0.0, 0.4, saturation);
    return mix(vec3<f32>(1.0), colored, color_influence);
}

fn liquid_rim_light(
    normal: vec3<f32>,
    distance: f32,
    height: f32,
    thickness: f32,
    light: vec4<f32>,
    background: vec3<f32>,
) -> vec3<f32> {
    let normalized_height = height / thickness;
    let edge_shape = clamp((1.0 - normalized_height) * 1.111, 0.0, 1.0);
    let thickness_visibility = clamp((thickness - 5.0) * 0.5, 0.0, 1.0);
    let rim_distance = distance / 1.5;
    let rim = 1.0 / (1.0 + 0.89 * rim_distance * rim_distance);

    let light_direction = light.xy / max(length(light.xy), 0.001);
    let main_light = max(dot(normal.xy, light_direction), 0.0);
    let opposite_light = max(dot(normal.xy, -light_direction), 0.0);
    let influence = main_light + opposite_light * 0.8;
    let highlight = liquid_highlight_color(background);
    let directional = highlight * 0.7 * influence * influence * light.z * 2.0;
    let ambient = highlight * 0.4 * light.w;
    return (directional + ambient) * rim * thickness_visibility * edge_shape;
}

fn backdrop_main(input: WgslInput, params: array<vec4<f32>, 4>) -> vec4<f32> {
    // tint: rgba
    let tint = params[0];
    // optical: refractive index, chromatic aberration, thickness px, blur px
    let optical = params[1];
    // light: direction xy, intensity, ambient strength
    let light = params[2];
    // geometry: canvas width, canvas height, saturation, corner radius px
    let geometry = params[3];

    let size = input.bounds.zw;
    let radius = clamp(geometry.w, 0.0, min(size.x, size.y) * 0.5);
    let thickness = max(optical.z, 0.001);
    let distance = liquid_rounded_rect_sdf(input.local_position, size, radius);
    let height = liquid_surface_height(distance, thickness);
    let normal = liquid_surface_normal(distance, thickness);

    let incident = vec3<f32>(0.0, 0.0, -1.0);
    let refracted = refract(incident, normal, 1.0 / max(optical.x, 1.001));
    let travel = (height + thickness * 8.0) / max(abs(refracted.z), 0.001);
    let displacement_px = refracted.xy * travel;
    let texel_size = vec2<f32>(1.0) / max(geometry.xy, vec2<f32>(1.0));
    let dispersion = optical.y * 0.5;

    let red_uv = input.screen_uv + displacement_px * (1.0 + dispersion) * texel_size;
    let green_uv = input.screen_uv + displacement_px * texel_size;
    let blue_uv = input.screen_uv + displacement_px * (1.0 - dispersion) * texel_size;
    let red = liquid_frost_sample(red_uv, texel_size, optical.w);
    let green = liquid_frost_sample(green_uv, texel_size, optical.w);
    let blue = liquid_frost_sample(blue_uv, texel_size, optical.w);

    var glass = vec3<f32>(red.r, green.g, blue.b);
    glass = liquid_apply_tint(glass, tint);
    glass += liquid_rim_light(normal, distance, height, thickness, light, glass);
    let luminance = dot(glass, LIQUID_LUMA);
    glass = mix(vec3<f32>(luminance), glass, geometry.z);

    return vec4<f32>(clamp(glass, vec3<f32>(0.0), vec3<f32>(1.0)), green.a);
}
"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("render-shapes.png"));
    let liquid_glass = WgslBackdrop::<4>::new("liquid glass", LIQUID_GLASS_WGSL)?;
    let mut surface = OffscreenSurface::new(SIZE).await.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no compatible WebGPU adapter found",
        )
    })?;
    let draws = scene(&liquid_glass);
    let rgba = surface.render_rgba8(&draws, color(9, 15, 32)).await;

    let file = File::create(&output)?;
    PngEncoder::new(file).write_image(&rgba, SIZE[0], SIZE[1], ExtendedColorType::Rgba8)?;

    println!("wrote {}", output.display());
    Ok(())
}

fn scene(liquid_glass: &WgslBackdrop<4>) -> DrawList {
    let mut draws = DrawList::new();
    let card = Rect::new(55.0, 40.0, 690.0, 420.0);
    let card_round = rounded(44.0);
    let glass = Rect::new(120.0, 135.0, 560.0, 230.0);
    let glass_round = rounded(58.0);

    draws.draw_rounded_rect(card, color(226, 232, 255), card_round);
    draws.with_rounded_clip(card, card_round, |draws| {
        draws.draw_rounded_rect(
            Rect::new(20.0, 75.0, 300.0, 300.0),
            color(99, 102, 241),
            rounded(150.0),
        );
        draws.draw_rounded_rect(
            Rect::new(500.0, 40.0, 300.0, 300.0),
            color(45, 212, 191),
            rounded(150.0),
        );
        draws.draw_rounded_rect(
            Rect::new(500.0, 315.0, 190.0, 190.0),
            color(251, 191, 36),
            rounded(95.0),
        );
        draws.draw_rounded_rect(
            Rect::new(195.0, 315.0, 210.0, 210.0),
            color(244, 114, 182),
            rounded(105.0),
        );

        let bar_colors = [
            color(244, 63, 94),
            color(56, 189, 248),
            color(167, 139, 250),
            color(251, 146, 60),
        ];
        for index in 0..9 {
            draws.draw_rounded_rect(
                Rect::new(98.0 + index as f32 * 72.0, 85.0, 28.0, 330.0),
                bar_colors[index % bar_colors.len()],
                rounded(14.0),
            );
        }
    });

    // A backdrop draw samples every command above it in the retained list.
    liquid_glass.draw_rounded(
        &mut draws,
        glass,
        glass_round,
        [
            [0.78, 0.92, 1.0, 0.14],
            [1.2, 0.18, 16.0, 2.5],
            [-0.707, -0.707, 0.72, 0.16],
            [SIZE[0] as f32, SIZE[1] as f32, 1.2, 58.0],
        ],
    );
    draws.draw_rounded_stroke(
        Stroke::from_rect(glass, 2.0),
        rgba(255, 255, 255, 0.72),
        glass_round,
    );

    // This opaque pill is recorded later, so it stays crisp on top of the glass.
    draws.draw_rounded_rect(
        Rect::new(285.0, 211.0, 230.0, 78.0),
        color(15, 23, 42),
        rounded(39.0),
    );
    draws.draw_rounded_stroke(
        Stroke::new(285.0, 211.0, 230.0, 78.0, 2.0),
        rgba(255, 255, 255, 0.9),
        rounded(39.0),
    );
    draws.draw_rounded_stroke(
        Stroke::from_rect(card, 3.0),
        rgba(255, 255, 255, 0.6),
        card_round,
    );

    draws
}

const fn rounded(radius: f32) -> Round {
    Round {
        lt: radius,
        rt: radius,
        rb: radius,
        lb: radius,
    }
}

const fn color(red: u8, green: u8, blue: u8) -> wgpu::Color {
    rgba(red, green, blue, 1.0)
}

const fn rgba(red: u8, green: u8, blue: u8, alpha: f64) -> wgpu::Color {
    wgpu::Color {
        r: red as f64 / 255.0,
        g: green as f64 / 255.0,
        b: blue as f64 / 255.0,
        a: alpha,
    }
}
