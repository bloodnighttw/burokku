# Phase 6 performance validation

Run the repeatable, headless CPU benchmarks with an optimized build:

```sh
cargo bench -p burokku --bench phase6
```

The benchmark emits tab-separated results for trees containing 100, 1,000,
and 10,000 application nodes. It covers:

- end-to-end commit latency;
- shallow snapshot creation and atomic publication latency;
- MTS `ArcSwap` snapshot loads;
- clean, partially dirty, and fully dirty layout;
- Vello scene construction; and
- MTS and BTS bounded macrotask queue submission under load, including the
  observed depth and dropped submissions in each measured batch.

The custom harness intentionally has no external benchmark dependency, so it
can compile in offline and restricted build environments. The workspace's
optimized benchmark profile enables debug assertions to include the diagnostic
metrics without adding them to production builds. Compare results from the same
host, power mode, and Rust toolchain. Multiple runs are recommended before
making an architectural change.

## DOM lifecycle reclamation

Measure JavaScript DOM checkpoint overhead with 100, 1,000, and 10,000 live
wrappers:

```sh
cargo bench -p burokku --bench dom_lifecycle
```

`clean_checkpoint` isolates the cost of a macrotask that does not mutate the
DOM. `attribute_checkpoint` includes an ordinary content mutation and snapshot
publication. This makes an accidental full DOM reclamation scan at every
checkpoint visible as tree size increases.

## Window/GPU measurements

GPU rendering, presentation, and commit-to-present latency require a real
native surface and are recorded by debug-only instrumentation rather than the
headless harness. Run a debug windowed application with metric output:

```sh
pnpm --filter @burokku/example-counter build
BUROKKU_PRINT_METRICS=1 cargo run -p burokku-example-counter
```

Exercise the application, then close the window. The final
`PerformanceMetricsSnapshot` reports latest and maximum values for:

- snapshot creation and publication;
- total frame, layout, scene construction, and Vello render/present time;
- commit-to-present latency;
- frame attempts and successfully presented frames;
- coalesced redraw requests and skipped committed revisions;
- events dropped by queue backpressure; and
- the BTS queue depth high-water mark.

These counters use relaxed atomics and do not introduce a DOM lock. They and
their timing calls are omitted when `debug_assertions` is disabled. Queue depth
is also available directly through `MacrotaskQueue::depth()` and
`MacrotaskQueue::max_capacity()` for MTS/BTS load tests.
