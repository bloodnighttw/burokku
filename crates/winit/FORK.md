# Burokku Winit fork

This directory vendors the upstream Winit `0.30.13` crate used by Burokku.

- Upstream: `https://github.com/rust-windowing/winit`
- Version: `0.30.13`
- Crates.io checksum: `a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d`

Burokku consumes it through the path dependency in
`crates/burokku/Cargo.toml`.

## macOS Metal patch

`WinitView` installs a `CAMetalLayer` as its root backing layer before WGPU
creates the surface. The layer uses `kCAGravityTopLeft` and the window backing
scale. This makes WGPU reuse the root Metal layer instead of creating its
default observer-managed child layer.
