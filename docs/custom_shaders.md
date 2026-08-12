# Custom WGSL shaders

The `render` crate provides two convenience renderers for custom WGSL:

- `WgslRaster` generates pixels without reading the existing scene.
- `WgslBackdrop` reads the scene produced by earlier draw commands and replaces
  pixels inside its bounds.

Both work with the same `DrawList`, clipping, ordering, `Canvas`, and
`OffscreenSurface` used by the built-in shape helpers.

## When to use a custom shader

Choose the smallest rendering API that provides the effect:

| Requirement | API |
| --- | --- |
| Solid or rounded fills and strokes | Built-in `draw_rect`, `draw_rounded_rect`, and stroke helpers |
| A procedural gradient, pattern, noise field, or SDF that does not read existing pixels | `WgslRaster` |
| Blur, tint, inversion, refraction, or another effect that reads existing pixels | `WgslBackdrop` |
| Custom textures, buffers, bind groups, geometry, or typed payloads | Implement `RasterRenderer` or `BackdropRenderer` |

Prefer the built-in helpers for ordinary UI geometry. They communicate intent
more clearly and can share optimized batches. Use custom WGSL when the visual
result is fundamentally easier to express per pixel.

Each backdrop draw copies the current internal scene and starts another render
pass. This preserves exact command ordering, but is more expensive than a
raster draw. Keep the number and bounds of backdrop effects modest, especially
when rendering several glass surfaces.

### When to use the liquid-glass shader

Liquid glass is appropriate for a bounded piece of UI chrome that should
visibly refract content behind it, such as a floating toolbar, control, or
overlay over a colorful or changing scene. It must be a `WgslBackdrop`, because
the effect depends on pixels drawn earlier in the frame.

If the design only needs a translucent rounded fill and border, use the
built-in shape helpers. A liquid-glass backdrop adds little visual value over a
flat background and still incurs the extra scene copy and pass.

## Shared shader contract

Create a shader registration once, retain it with the rest of the application
or renderer state, and use it to record draws each frame. `new` parses and
validates the composed WGSL immediately. Its GPU pipeline is then created
lazily once for each render target that encounters the registration.

The engine injects the following input type. Do not redeclare it in custom
source:

```wgsl
struct WgslInput {
    // Pixels relative to the draw's top-left corner.
    local_position: vec2<f32>,
    // Coordinates from 0 to 1 within the draw bounds.
    local_uv: vec2<f32>,
    // Pixel coordinates in the canvas.
    pixel_position: vec2<f32>,
    // Coordinates from 0 to 1 across the canvas.
    screen_uv: vec2<f32>,
    // Draw x, y, width, and height in pixels.
    bounds: vec4<f32>,
};
```

`WgslRaster<N>` requires this function:

```wgsl
fn raster_main(
    input: WgslInput,
    params: array<vec4<f32>, N>,
) -> vec4<f32>;
```

`WgslBackdrop<N>` instead requires:

```wgsl
fn backdrop_main(
    input: WgslInput,
    params: array<vec4<f32>, N>,
) -> vec4<f32>;
```

Here, `N` describes the contract rather than literal WGSL: replace it with the
same positive integer in the Rust type and shader source. Rust parameters have
type `[[f32; 4]; N]` and map directly to WGSL's
`array<vec4<f32>, N>`. Give each slot a documented meaning so the call site
remains readable.

Custom source may contain constants and helper functions. The engine owns the
vertex and fragment entry points, bounded quad geometry, antialiased rounded
coverage, and active clip masks.

## Procedural drawing with `WgslRaster`

This two-parameter shader draws a vertical gradient:

```rust
use render::{
    canvas::DrawList,
    shapes::{rect::Rect, round::Round},
    wgsl::WgslRaster,
};

const VERTICAL_GRADIENT: &str = r#"
fn raster_main(input: WgslInput, params: array<vec4<f32>, 2>) -> vec4<f32> {
    return mix(params[0], params[1], input.local_uv.y);
}
"#;

let gradient = WgslRaster::<2>::new("vertical gradient", VERTICAL_GRADIENT)?;
let mut draws = DrawList::new();

gradient.draw_rounded(
    &mut draws,
    Rect::new(40.0, 40.0, 240.0, 160.0),
    Round {
        lt: 24.0,
        rt: 24.0,
        rb: 24.0,
        lb: 24.0,
    },
    [
        [0.20, 0.40, 1.00, 1.00], // top RGBA
        [0.80, 0.20, 0.90, 1.00], // bottom RGBA
    ],
);
```

The returned color uses straight alpha and is blended over preceding commands.
The engine multiplies its alpha by the draw's rounded and clip coverage.

Use `draw` instead of `draw_rounded` when the custom output has rectangular
bounds.

## Sampling earlier draws with `WgslBackdrop`

A backdrop shader can call `sample_backdrop(screen_uv)`. The helper uses a
linearly filtered sampler and clamps coordinates to the canvas edge.

The following shader tints the existing scene:

```rust
use render::{
    canvas::DrawList,
    shapes::{
        rect::{DrawRectExt, Rect},
        round::Round,
    },
    wgpu,
    wgsl::WgslBackdrop,
};

const SCENE_TINT: &str = r#"
fn backdrop_main(input: WgslInput, params: array<vec4<f32>, 1>) -> vec4<f32> {
    let prior = sample_backdrop(input.screen_uv);
    let amount = clamp(params[0].a, 0.0, 1.0);
    return vec4<f32>(
        mix(prior.rgb, prior.rgb * params[0].rgb, amount),
        prior.a,
    );
}
"#;

let tint = WgslBackdrop::<1>::new("scene tint", SCENE_TINT)?;
let mut draws = DrawList::new();
let effect_bounds = Rect::new(40.0, 40.0, 240.0, 160.0);
let effect_round = Round {
    lt: 24.0,
    rt: 24.0,
    rb: 24.0,
    lb: 24.0,
};

// This shape is part of the scene sampled by the effect.
draws.draw_rect(Rect::new(0.0, 0.0, 320.0, 240.0), wgpu::Color::BLUE);

tint.draw_rounded(
    &mut draws,
    effect_bounds,
    effect_round,
    [[0.70, 0.90, 1.00, 0.35]], // tint RGB and amount
);

// This shape is recorded later and remains unaffected above the tint.
draws.draw_rounded_rect(
    Rect::new(100.0, 90.0, 120.0, 60.0),
    wgpu::Color::WHITE,
    effect_round,
);
```

Command order is part of the effect:

```text
background raster -> backdrop samples it -> foreground raster draws on top
```

A backdrop sees only the clear color and commands before it. Consecutive
backdrop draws see the latest scene, including the result of earlier backdrop
effects.

The value returned by `backdrop_main` is the final affected pixel, not an
overlay color that will be alpha-blended automatically. At antialiased rounded
or clipped edges, the engine mixes that value with the unmodified scene pixel.

## Using the liquid-glass example

The complete liquid-glass shader and scene are in
[`crates/render/examples/shapes_to_png.rs`](../crates/render/examples/shapes_to_png.rs).
It adapts the SDF refraction, dispersion, tint, saturation, and rim-lighting
approach from the MIT-licensed
[`whynotmake-it/flutter_liquid_glass`](https://github.com/whynotmake-it/flutter_liquid_glass)
project. The example retains the upstream license notice beside the shader.

Run it from the workspace root:

```sh
cargo run -p render --example shapes_to_png -- ./liquid-glass.png
```

The important integration pattern is shorter than the shader itself:

```rust
let liquid_glass =
    WgslBackdrop::<4>::new("liquid glass", LIQUID_GLASS_WGSL)?;

// These commands become the sampled backdrop.
draw_background(&mut draws);

liquid_glass.draw_rounded(
    &mut draws,
    glass_bounds,
    glass_round,
    [
        [0.78, 0.92, 1.00, 0.14],
        [1.20, 0.18, 16.0, 2.50],
        [-0.707, -0.707, 0.72, 0.16],
        [canvas_width, canvas_height, 1.20, 58.0],
    ],
);

// These commands stay crisp above the glass.
draw_foreground(&mut draws);
```

The liquid-glass parameter slots are:

| Slot | Components |
| --- | --- |
| `params[0]` | Tint red, green, blue, and amount |
| `params[1]` | Refractive index, chromatic aberration, thickness in pixels, and frost-blur radius in pixels |
| `params[2]` | Light direction x/y, directional intensity, and ambient intensity |
| `params[3]` | Canvas width/height, saturation, and optical corner radius in pixels |

Pass the current canvas dimensions because the shader converts its pixel-space
refraction displacement into normalized texture coordinates. Keep the optical
corner radius in `params[3].w` aligned with `glass_round`: the parameter shapes
the refracted surface, while `glass_round` clips the final output.

## Clipping

Custom draws participate in normal clip scopes. The engine centrally resolves
rectangular scissors and rounded masks, so WGSL source should not reproduce the
clip stack:

```rust
draws.with_rounded_clip(clip_bounds, clip_round, |draws| {
    liquid_glass.draw_rounded(draws, glass_bounds, glass_round, params);
});
```

Prefer the scoped helpers. If manual `push_clip` and `pop_clip` calls are
unbalanced, window rendering returns `CanvasRenderError::UnbalancedClipStack`;
offscreen rendering treats it as a programming error.

## Lifetime, errors, and limits

- Keep `WgslRaster` and `WgslBackdrop` registrations long-lived. Cloning one
  shares the same registration; recreating it every frame defeats pipeline
  reuse.
- Propagate or report `WgslError` from `new`. It rejects zero parameter slots,
  impossible parameter layouts, invalid WGSL, missing entry functions, and
  signature mismatches.
- The convenience API deliberately exposes only a fixed array of `vec4`
  parameters. Implement the lower-level renderer traits when an effect needs
  its own textures, buffers, bind groups, geometry, or richer typed payloads.
- One custom backdrop shader is one pass over the prior scene. Chain ordered
  backdrop commands for a small multi-stage effect; effects needing a larger
  render graph should be implemented in or alongside the compositor.
