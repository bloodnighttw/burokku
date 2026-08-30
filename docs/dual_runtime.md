# Superseded: Dual Runtime Architecture

This document described the removed dual-runtime design. Burokku now uses one thread-affine QuickJS runtime on the UI thread; there is no background JavaScript isolate or runtime bridge.

See [`dom_layout_colocation_plan.md`](dom_layout_colocation_plan.md) for the implemented architecture and migration record.
