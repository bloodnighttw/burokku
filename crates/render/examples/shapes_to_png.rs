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
    wgsl::WgslBlurredBackdrop,
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

The renderer runs the same three logical stages as upstream: separable Gaussian
backdrop blur, displacement geometry, and final glass composition. Geometry and
composition are combined in the final WGSL pass because this example has one
static shape and does not need upstream's reusable geometry-texture cache.
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

fn backdrop_main(input: WgslInput, params: array<vec4<f32>, 4>) -> vec4<f32> {
    // tint: rgba
    let tint = params[0];
    // optical: refractive index, chromatic aberration, thickness px, unused
    let optical = params[1];
    // light: direction xy, intensity, ambient strength
    let light = params[2];
    // geometry: canvas width, canvas height, saturation, corner radius px
    let geometry = params[3];

    let size = input.bounds.zw;
    let radius = clamp(geometry.w, 0.0, min(size.x, size.y) * 0.5);
    let thickness = max(optical.z, 0.001);
    let distance = liquid_rounded_rect_sdf(input.local_position, size, radius);
    let foreground_alpha = select(
        0.0,
        1.0 - smoothstep(-2.0, 0.0, distance),
        distance < 0.0,
    );

    let edge_depth = thickness + distance;
    let curved_height = sqrt(max(0.0, thickness * thickness - edge_depth * edge_depth));
    let height = select(curved_height, thickness, distance < -thickness);
    let gradient = vec2<f32>(dpdx(distance), dpdy(distance));
    let normal_xy = max(thickness + distance, 0.0) / thickness;
    let normal_z = sqrt(max(0.0, 1.0 - normal_xy * normal_xy));
    let normal = normalize(vec3<f32>(gradient * normal_xy, normal_z));

    let incident = vec3<f32>(0.0, 0.0, -1.0);
    let refracted_ray = refract(incident, normal, 1.0 / max(optical.x, 1.001));
    let travel = (height + thickness * 8.0) / max(abs(refracted_ray.z), 0.001);
    let displacement_px = refracted_ray.xy * travel;
    let texel_size = vec2<f32>(1.0) / max(geometry.xy, vec2<f32>(1.0));
    let dispersion = optical.y * 0.5;

    let red_uv = input.screen_uv + displacement_px * (1.0 + dispersion) * texel_size;
    let green_uv = input.screen_uv + displacement_px * texel_size;
    let blue_uv = input.screen_uv + displacement_px * (1.0 - dispersion) * texel_size;
    let red = sample_backdrop(red_uv);
    let green = sample_backdrop(green_uv);
    let blue = sample_backdrop(blue_uv);
    let refracted = vec4<f32>(red.r, green.g, blue.b, green.a);

    // Port of liquid_glass_final_render.frag from the current renderer.
    var glass = tint.rgb * tint.a + refracted.rgb * (1.0 - tint.a);
    let luminance = dot(glass, LIQUID_LUMA);
    glass = clamp(mix(vec3<f32>(luminance), glass, geometry.z), vec3<f32>(0.0), vec3<f32>(1.0));

    let normalized_height = height / thickness;
    let thickness_scale = clamp(40.0 / max(thickness, 1.0), 1.0, 4.0);
    let edge_threshold = mix(0.8, 0.5, 1.0 / thickness_scale);
    let edge_factor = 1.0 - smoothstep(0.0, edge_threshold, normalized_height);
    if edge_factor > 0.01 {
        let displacement_length = length(displacement_px);
        let edge_normal = displacement_px / max(displacement_length, 0.001);
        let light_direction = light.xy / max(length(light.xy), 0.001);
        let main_light = max(dot(edge_normal, light_direction), 0.0);
        let opposite_light = max(dot(edge_normal, -light_direction), 0.0);
        let influence = main_light + opposite_light * 0.8;
        let directional = pow(influence, 1.5) * light.z * 3.0;
        let ambient = light.w * 0.5;
        let brightness = (directional + ambient) * edge_factor * thickness_scale * 0.8;

        let background_luminance = dot(refracted.rgb, LIQUID_LUMA);
        var saturated_background = refracted.rgb / max(background_luminance, 0.001);
        saturated_background = mix(refracted.rgb, saturated_background, 0.8);
        let colorfulness = length(refracted.rgb - vec3<f32>(background_luminance));
        let color_mix = clamp(colorfulness + 0.5, 0.5, 1.0);
        let highlight = mix(vec3<f32>(1.0), saturated_background, color_mix);
        glass = mix(glass, highlight, brightness);
    }

    return vec4<f32>(
        clamp(glass, vec3<f32>(0.0), vec3<f32>(1.0)),
        foreground_alpha,
    );
}
"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("render-shapes.png"));
    let liquid_glass = WgslBlurredBackdrop::<4>::new("liquid glass", LIQUID_GLASS_WGSL)?;
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

fn scene(liquid_glass: &WgslBlurredBackdrop<4>) -> DrawList {
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
        5.0,
        [
            [1.0, 1.0, 1.0, 0.0],
            [1.2, 0.01, 20.0, 0.0],
            [0.0, 1.0, 0.5, 0.0],
            [SIZE[0] as f32, SIZE[1] as f32, 1.5, 58.0],
        ],
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
