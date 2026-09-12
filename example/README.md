# Examples

The high-level `Burokku` runner used below installs the DOM and JavaScript
`ResizeObserver` bindings automatically. Code that builds the JavaScript runtime
directly installs the observer plugin after the DOM plugin:

```rust
use burokku::{
    plugins::{dom::DomPlugin, resize_observer::ResizeObserverPlugin},
    RuntimeBuilder,
};

let builder = RuntimeBuilder::new()
    .plugin(DomPlugin::new())
    .plugin(ResizeObserverPlugin);
```

Standalone installation exposes the JS API; it does not create a native window
or compute layout. Observations require a native host supplying completed layouts
for the existing DOM.

## Resize observation

```js
const win = app.createElement("window");
const panel = app.createElement("div");
panel.style.setProperty("width", "50%");
win.appendChild(panel);
app.appendChild(win);

const observer = new ResizeObserver(entries => {
  for (const entry of entries) {
    console.log(entry.target.localName, entry.size.width, entry.size.height);
  }
});
observer.observe(win);
observer.observe(panel);

// Later: observer.unobserve(panel) or observer.disconnect().
```

Entries contain immutable layout sizes in logical pixels. Delivery is asynchronous
and is not guaranteed before paint. Box-selection options are future work.
`@burokku/runtime` declares the global constructor and exports the
`BurokkuResizeObserver`, `BurokkuResizeObserverEntry`, and
`BurokkuResizeObserverCallback` types.

## Event dispatch showcase

`example/events` renders an interactive div and displays click, pointer, wheel,
keyboard, bubbling, and pointer-capture events as they are dispatched. A single
resize observer watches both the div and the native window, showing their current
layout sizes in the UI and logging changes. Resize the window to see both update.

```sh
cargo run -p burokku-example-events
```

For a bounded smoke run:

```sh
BUROKKU_SMOKE=1 cargo run -p burokku-example-events
```

## LLRT counter

`example/counter` is a Rust binary that embeds a JavaScript application and uses
LLRT's `setInterval` to update a live DOM text node once per second.

```sh
cargo run -p burokku-example-counter
```

For a bounded smoke run, remove the Window after the first counter tick:

```sh
BUROKKU_SMOKE=1 cargo run -p burokku-example-counter
```

## Layout showcase

`example/layouts` embeds `src/app.js`, registers the bundled Noto Sans fixture,
and presents flex layout, inherited text runs, backgrounds, and wrapped
paragraphs through Taffy, Parley, Vello Hybrid, and WGPU.

```sh
cargo run -p burokku-example-layouts
```

A bounded smoke run is also available:

```sh
BUROKKU_SMOKE=1 cargo run -p burokku-example-layouts
```

## Positioned mixed-layout showcase

`example/positioning` mixes a flex shell, relative grid, static block wrapper,
absolute overlay, and fixed footer. The source includes an XML-like view of the
authoritative DOM so its parentage can be compared with the rendered layout.

```sh
cargo run -p burokku-example-positioning
```

For a bounded smoke run:

```sh
BUROKKU_SMOKE=1 cargo run -p burokku-example-positioning
```
