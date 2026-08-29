# Examples

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
