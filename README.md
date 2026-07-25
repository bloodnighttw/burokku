# Burokku

A Rust workspace containing an asynchronous JavaScript runtime and the Burokku application.

## Layout

- `crates/runtime` — rquickjs-based JavaScript runtime with Tokio integration
- `crates/render` — surface-backed WebGPU drawing library for boxes and text
- `crates/winit` — small macOS/AppKit windowing layer with an async Tokio driver
- `crates/burokku` — DOM-like API, Taffy layout, and application host
- `packages/runtime` — shared DOM property and style operations
- `packages/react` — custom React reconciler for Burokku nodes
- `packages/solid` — custom Solid universal renderer for Burokku nodes
- `example/react` and `example/solid` — Vite examples compiled for QuickJS

## Drawing

`burokku` owns the native window and WebGPU surface. `render` owns the drawing
pipelines, shader code, text shaping, and texture atlas, and renders into the
surface supplied by the application:

```rust
use render::{
    wgpu, Border, BoxStyle, Canvas, Color, CornerRadius, Outline, Rect,
    Renderer, SurfaceSize, TextConstraints, TextStyle, TextSystem,
};

// `window` is an Arc<burokku_winit::Window> on the AppKit main thread.
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

Burokku currently supports macOS. Its AppKit event loop stays on the main
thread inside a Tokio runtime, while JavaScript futures, timers, I/O, and other
Tokio tasks can run on worker threads. AppKit delegate notifications dispatch
immediately, including from the nested event-tracking loop used during live
window resizing.

```sh
cargo build --workspace
cargo run -p burokku
```

The default script draws a small DOM example. Pass a compiled JavaScript file as
the first argument to run it instead:

```sh
cargo run -p burokku -- ./script.js
```

## DOM UI

Burokku installs `document`, `Node`, `Element`, `HTMLElement`, `Text`, and
`DocumentFragment` in QuickJS. JavaScript can use familiar DOM operations such
as `document.createElement`, `appendChild`, `insertBefore`, `removeChild`,
`textContent`, attributes, and `element.style`. Rust owns the persistent tree,
measures text with `TextSystem`, computes layout with Taffy, then converts the
result into `render::Canvas` drawing commands.

React uses the custom reconciler in `@burokku/react`:

```tsx
import { createRoot } from "@burokku/react";

createRoot(document.body).render(
  <div style={{ padding: 24, backgroundColor: "#f5f7fa" }}>Hello</div>,
);
```

Solid uses `@burokku/solid`, which is built on `solid-js/universal` and performs
fine-grained updates against the same native nodes:

```tsx
import { render } from "@burokku/solid";

render(() => <div style={{ padding: "24px" }}>Hello</div>, document.body);
```

The currently supported visual CSS subset is flex/block display, width and
height constraints, gap, padding, margin, clipped and scrollable overflow,
background and text color, border, outline, radius, and basic font properties.
Scrollable boxes respond to mouse-wheel input, scrollbar track clicks, and
thumb dragging.

Build and run the Vite example with:

```sh
pnpm install
pnpm --filter @burokku/example-react build
cargo run -p burokku -- example/react/dist/app.js

pnpm --filter @burokku/example-solid build
cargo run -p burokku -- example/solid/dist/app.js
```

Each command opens an `800x600` native window and presents the UI through
WebGPU.

Native close, resize, scale-factor, focus, occlusion, keyboard, modifier,
cursor, mouse-button, and wheel events are delivered to
`window` event listeners. Element-level hit testing and event bubbling are not
connected yet.

Run checks with:

```sh
just test
```

`just test` runs the Rust workspace tests, type-checks all three runtime
packages, builds both Vite examples, and executes both bundles through QuickJS.
