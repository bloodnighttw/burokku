# Vello shapes

A small, self-contained example that opens a native window and draws filled and
stroked vector shapes with [Vello](https://github.com/linebender/vello).

Run it from the repository root:

```sh
cargo run -p vello-shapes
```

The drawing code is in `add_shapes_to_scene` in `src/main.rs`. Change that
function to experiment with `Circle`, `RoundedRect`, `BezPath`, fills, strokes,
colors, and transforms.
