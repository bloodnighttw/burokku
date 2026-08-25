# Layout example

`example/layouts` is a Rust binary that embeds `dist/app.js`, installs the
Burokku application host, registers the bundled Noto Sans fixture, and presents
the resulting Window through Taffy, Parley, Vello Hybrid, and WGPU.

Run it from the workspace root:

```sh
cargo run -p burokku-example-layouts
```

For a bounded manual smoke check, set `BUROKKU_SMOKE`; the script removes its
Window after a short interval and the host exits cleanly:

```sh
BUROKKU_SMOKE=1 cargo run -p burokku-example-layouts
```

The checked-in JavaScript bundle exercises flex layout, nested inherited text
runs, backgrounds, and multiple wrapped paragraphs.
