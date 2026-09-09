# Examples

The high-level `Burokku` runner used below installs the DOM bindings
automatically. Code that builds the JavaScript runtime directly can install the
moved plugin explicitly:

```rust
use burokku::{plugins::dom::DomPlugin, RuntimeBuilder};

let builder = RuntimeBuilder::new().plugin(DomPlugin::new());
```

## Event dispatch showcase

`example/events` renders an interactive UI element and displays click, pointer,
wheel, keyboard, bubbling, and pointer-capture events as they are dispatched.

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
