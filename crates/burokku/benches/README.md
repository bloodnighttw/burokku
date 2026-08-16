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
can compile in offline and restricted build environments. Compare optimized
results from the same host, power mode, and Rust toolchain. Multiple runs are
recommended before making an architectural change.

## Window/GPU measurements

GPU rendering, presentation, and commit-to-present latency require a real
native surface and are recorded by production instrumentation rather than the
headless harness. Run an optimized windowed application with metric output:

```sh
BUROKKU_PRINT_METRICS=1 cargo run --release -p burokku -- example/counter.js
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

These counters use relaxed atomics and do not introduce a DOM lock. Queue depth
is also available directly through `MacrotaskQueue::depth()` and
`MacrotaskQueue::max_capacity()` for MTS/BTS load tests.
