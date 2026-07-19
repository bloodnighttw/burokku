# Burokku

A Rust workspace containing an asynchronous JavaScript runtime and the Burokku application.

## Layout

- `crates/runtime` — rquickjs-based JavaScript runtime with Tokio integration
- `crates/render` — surface-backed WebGPU drawing library for boxes and text
- `crates/burokku` — application that runs JavaScript through the runtime

## Drawing

`burokku` owns the `winit` window and WebGPU surface. `render` owns the drawing
pipelines, shader code, text shaping, and texture atlas, and renders into the
surface supplied by the application:

```rust
use render::{
    wgpu, Border, BoxStyle, Canvas, Color, CornerRadius, Outline, Rect,
    Renderer, SurfaceSize, TextStyle,
};

// `window` is owned by the winit application.
let instance = wgpu::Instance::new(
    wgpu::InstanceDescriptor::new_without_display_handle(),
);
let surface = instance.create_surface(window.clone())?;
let window_size = window.inner_size();
let size = SurfaceSize::new(window_size.width, window_size.height);
let mut renderer = Renderer::new(&instance, &surface, size).await?;

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
canvas.draw_text(
    Rect::new(64.0, 64.0, 512.0, 80.0),
    "Drawn without application WGSL",
    TextStyle {
        font_size: 28.0,
        line_height: 34.0,
        ..TextStyle::default()
    },
);

renderer.render(&surface, &canvas)?;

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

Run checks with:

```sh
cargo test --workspace
```

The same workflows are available through `just build`, `just run`, and `just test`.
