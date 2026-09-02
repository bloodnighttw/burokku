# Burokku UI Architecture

The active architecture is documented in [`../../docs/dom_layout_colocation_plan.md`](../../docs/dom_layout_colocation_plan.md).

Burokku uses one thread-affine QuickJS runtime and one live DOM on the native UI thread. The platform event loop owns the process main thread and polls one persistent `LocalSet`; upstream Tokio's worker owns timers, I/O, and `Send` tasks. JavaScript mutations complete before `about_to_wait`; layout and owned scene construction borrow the live DOM only for the frame-building scope, then release it before native or GPU presentation work.
