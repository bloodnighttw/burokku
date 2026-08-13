# Vello liquid-glass stress test

A self-contained example that renders a colorful vector backdrop with
[Vello](https://github.com/linebender/vello), then composites a configurable
number of overlapping liquid-glass panes in one instanced WGSL draw.

Run it from the repository root:

```sh
cargo run -p vello-shapes
```

The terminal prints the CPU time used to encode, submit, and present every
frame. The animation redraws continuously so the timing is easy to observe.

Change `GLASS_COUNT` at the top of `src/liquid_glass.rs` to any practical `u32`
value. The instance generator derives the row and column count automatically;
zero and partially filled final rows are supported too.

- `src/main.rs` owns the Vello backdrop, window, and frame loop.
- `src/liquid_glass.rs` creates the wgpu pipeline and requested instances.
- `src/liquid_glass.wgsl` implements the rounded SDF, refraction, chromatic
  dispersion, tint, and rim lighting.

The optical model is adapted from the MIT-licensed
[`whynotmake-it/flutter_liquid_glass`](https://github.com/whynotmake-it/flutter_liquid_glass)
project. See `THIRD_PARTY_NOTICES.md`.
