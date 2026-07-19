# Burokku

A Rust workspace containing an asynchronous JavaScript runtime and the Burokku application.

## Layout

- `crates/runtime` — rquickjs-based JavaScript runtime with Tokio integration
- `crates/render` — surface-backed WebGPU drawing library for boxes and text
- `crates/burokku` — Taffy-based UI layout and application host
- `packages/ui` — host protocol and React renderer
- `example/react` — React + Vite example compiled for the QuickJS host

## Drawing

`burokku` owns the `winit` window and WebGPU surface. `render` owns the drawing
pipelines, shader code, text shaping, and texture atlas, and renders into the
surface supplied by the application:

```rust
use render::{
    wgpu, Border, BoxStyle, Canvas, Color, CornerRadius, Outline, Rect,
    Renderer, SurfaceSize, TextConstraints, TextStyle, TextSystem,
};

// `window` is owned by the winit application.
let instance = wgpu::Instance::new(
    wgpu::InstanceDescriptor::new_without_display_handle(),
);
let surface = instance.create_surface(window.clone())?;
let window_size = window.inner_size();
let size = SurfaceSize::new(window_size.width, window_size.height);
let mut renderer = Renderer::new(&instance, &surface, size).await?;
let mut text_system = TextSystem::new();

let mut canvas = Canvas::new()
    .with_clear_color(Color::from_rgba8(245, 247, 250, 255));
canvas.draw_box(
    Rect::new(32.0, 32.0, 576.0, 296.0),
    BoxStyle {
        background: Color::WHITE,
        corner_radius: CornerRadius::all(18.0),
        border: Some(Border::new(2.0, Color::from_rgba8(40, 45, 55, 255))),
        outline: Some(Outline::new(
            3.0,
            4.0,
            Color::from_rgba8(90, 120, 255, 180),
        )),
    },
);
let text_style = TextStyle {
    font_size: 28.0,
    line_height: 34.0,
    ..TextStyle::default()
};
let text_size = text_system.measure(
    "Measured for Taffy, rendered by Glyphon",
    &text_style,
    TextConstraints::at_most(512.0),
);
canvas.draw_text(
    Rect::new(64.0, 64.0, text_size.width, text_size.height),
    "Measured for Taffy, rendered by Glyphon",
    text_style,
);

renderer.render(&surface, &canvas, &mut text_system)?;

// On WindowEvent::Resized:
renderer.resize(&surface, SurfaceSize::new(new_size.width, new_size.height));
```

## Getting started

```sh
cargo build --workspace
cargo run -p burokku
```

The application prints a greeting and the result of a JavaScript calculation. Pass a
JavaScript file as the first argument to run it instead:

```sh
cargo run -p burokku -- ./script.js
```

## React UI

The React API maps `div`, `button`, `span`, and `text` to Burokku UI nodes. Set
`jsxImportSource` to `@burokku/ui` so TypeScript checks styles against the
Burokku API. At each React commit, the reconciler sends only typed create,
style, text, insert, and remove mutations to the Rust host, followed by one
flush marker. Rust owns the persistent UI tree, measures text with `TextSystem`,
computes layout with Taffy, then converts it into `render::Canvas` drawing
commands. No JSON tree serialization is involved.

Build and run the Vite example with:

```sh
pnpm install
pnpm --filter @burokku/example-react build
cargo run -p burokku -- example/react/dist/app.js
```

This opens an `800x600` native window and presents the React UI through WebGPU.

Set `BUROKKU_PERF=1` to print timing for every native stage to the terminal:

```text
[Burokku perf] React commit #1: bridge 0.092 ms (79 mutations)
[Burokku perf] React root render: 0.832 ms (reconcile + commit)
[Burokku perf] Host commit #1: applied 79 native mutations
[Burokku perf] UI commit #1 (initial): layout 0.410 ms, paint 0.021 ms, 8 commands
[Burokku perf] WebGPU frame #1 (commit #1): 0.350 ms CPU submit + present
```

The WebGPU number measures CPU preparation, command submission, and the call to
present the surface. It intentionally does not claim to be GPU execution time;
measuring that requires timestamp queries or waiting for the GPU, which would
change the behavior being measured.

The current bridge covers rendering and layout. Window/input event dispatch is
not part of this React API yet.

Run checks with:

```sh
just test
```

`just test` runs the Rust workspace tests, React renderer tests and type checks,
builds the Vite example, and executes that bundle through QuickJS.
