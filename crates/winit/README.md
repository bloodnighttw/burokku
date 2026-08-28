# burokku

This project provides a window for wgpu applications with Tokio integration.

It is inspired by winit's application-handler API. `EventLoop::run_app_external`
lets a native main loop drive a patched Tokio current-thread runtime and
`LocalSet`. The external-loop backend is currently implemented on macOS; other
platforms can implement the same wake/timer interface without changing callers.

The external-loop API uses this repository's patched Tokio. A downstream root
that consumes `burokku-winit` outside this workspace must apply the same
`[patch.crates-io] tokio = ...` override; Cargo patches are not transitive.

