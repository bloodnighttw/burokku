use std::{hint::black_box, time::Duration, time::Instant};

use burokku::ui::{
    computed::ComputedState,
    elements::{
        styles::{flex::FlexStyle, CommonStyle},
        BtsDom, Elements, NodeId, SharedDom,
    },
    frame::SceneState,
};
use runtime::{MacrotaskQueueError, Runtime, RuntimeRole};
use taffy::{geometry::Size, AvailableSpace};
use winit::PhysicalSize;

const TREE_SIZES: [usize; 3] = [100, 1_000, 10_000];
const VIEWPORT_WIDTH: f32 = 1_280.0;
const VIEWPORT_HEIGHT: f32 = 720.0;

fn main() {
    println!("benchmark\ttree_size\titerations\ttotal_ms\tns_per_iteration");
    commit_and_snapshot_benchmarks();
    layout_benchmarks();
    scene_construction_benchmarks();
    queue_load_benchmarks();
    eprintln!(
        "GPU rendering and commit-to-present are measured in a real window; see benches/README.md"
    );
}

fn report(name: &str, size: usize, iterations: u64, elapsed: Duration) {
    let ns_per_iteration = elapsed.as_nanos() as f64 / iterations as f64;
    println!(
        "{name}\t{size}\t{iterations}\t{:.3}\t{ns_per_iteration:.2}",
        elapsed.as_secs_f64() * 1_000.0,
    );
}

fn measure<T>(name: &str, size: usize, iterations: u64, mut operation: impl FnMut() -> T) {
    black_box(operation());
    let started = Instant::now();
    for _ in 0..iterations {
        black_box(operation());
    }
    report(name, size, iterations, started.elapsed());
}

fn measure_recorded(
    name: &str,
    size: usize,
    iterations: u64,
    mut operation: impl FnMut() -> Duration,
) {
    black_box(operation());
    let mut elapsed = Duration::ZERO;
    for _ in 0..iterations {
        elapsed += black_box(operation());
    }
    report(name, size, iterations, elapsed);
}

fn iterations_for(size: usize) -> u64 {
    match size {
        0..=100 => 10_000,
        101..=1_000 => 1_000,
        _ => 100,
    }
}

fn available_space() -> Size<AvailableSpace> {
    Size {
        width: AvailableSpace::Definite(VIEWPORT_WIDTH),
        height: AvailableSpace::Definite(VIEWPORT_HEIGHT),
    }
}

fn build_tree(size: usize) -> (SharedDom, BtsDom, Vec<NodeId>) {
    let shared = SharedDom::new();
    let mut owner = BtsDom::new(shared.clone());
    let mut nodes = Vec::with_capacity(size);
    {
        let mut dom = owner.mutate();
        let root = dom.root();
        let window = dom.create(Elements::Window {
            style: Box::default(),
        });
        dom.append_child(root, window).unwrap();
        for _ in 0..size {
            let node = dom.create(Elements::Div {
                style: Box::default(),
            });
            dom.append_child(window, node).unwrap();
            nodes.push(node);
        }
    }
    owner.checkpoint().unwrap().unwrap();
    (shared, owner, nodes)
}

fn mutate_one(owner: &mut BtsDom, target: NodeId, generation: u64) {
    owner
        .mutate()
        .set_attribute(target, "data-bench".into(), generation.to_string())
        .unwrap();
}

fn commit_and_snapshot_benchmarks() {
    for size in TREE_SIZES {
        let (shared, _owner, _nodes) = build_tree(size);
        measure("mts_snapshot_load", size, 100_000, || shared.load());
    }

    for size in TREE_SIZES {
        let (_shared, mut owner, nodes) = build_tree(size);
        let target = nodes[0];
        let mut generation = 0_u64;
        measure("commit_end_to_end", size, iterations_for(size), || {
            generation = generation.wrapping_add(1);
            mutate_one(&mut owner, target, generation);
            owner.checkpoint().unwrap().unwrap()
        });
    }

    // Production instrumentation times the snapshot clone and ArcSwap/watch
    // publication separately, avoiding benchmark-harness overhead in each
    // reported sub-step.
    for size in TREE_SIZES {
        let (shared, mut owner, nodes) = build_tree(size);
        let target = nodes[0];
        let mut generation = 0_u64;
        measure_recorded("snapshot_creation", size, iterations_for(size), || {
            generation = generation.wrapping_add(1);
            mutate_one(&mut owner, target, generation);
            black_box(owner.checkpoint().unwrap().unwrap());
            shared.metrics().snapshot().latest_snapshot_creation
        });
    }

    for size in TREE_SIZES {
        let (shared, mut owner, nodes) = build_tree(size);
        let target = nodes[0];
        let mut generation = 0_u64;
        measure_recorded("snapshot_publication", size, iterations_for(size), || {
            generation = generation.wrapping_add(1);
            mutate_one(&mut owner, target, generation);
            black_box(owner.checkpoint().unwrap().unwrap());
            shared.metrics().snapshot().latest_publication
        });
    }
}

fn layout_benchmarks() {
    for size in TREE_SIZES {
        let (shared, _owner, _nodes) = build_tree(size);
        let snapshot = shared.load();
        let mut computed = ComputedState::new();
        computed.compute_layout(&snapshot, available_space());
        measure("layout_clean", size, 100_000, || {
            computed.compute_layout(&snapshot, available_space())
        });
    }

    for size in TREE_SIZES {
        let (_shared, mut owner, nodes) = build_tree(size);
        let target = nodes[0];
        let mut generation = 0_u64;
        let mut computed = ComputedState::new();
        measure_recorded("layout_partially_dirty", size, iterations_for(size), || {
            generation = generation.wrapping_add(1);
            mutate_one(&mut owner, target, generation);
            let snapshot = owner.checkpoint().unwrap().unwrap();
            let started = Instant::now();
            black_box(computed.compute_layout(&snapshot, available_space()));
            started.elapsed()
        });
    }

    for size in TREE_SIZES {
        let (_shared, mut owner, nodes) = build_tree(size);
        let mut generation = 0_u64;
        let mut computed = ComputedState::new();
        measure_recorded("layout_fully_dirty", size, iterations_for(size), || {
            generation = generation.wrapping_add(1);
            let mut dom = owner.mutate();
            for node in &nodes {
                dom.set_element(
                    *node,
                    Elements::Flex {
                        style: Box::new(FlexStyle {
                            common: CommonStyle {
                                flex_grow: generation as f32,
                                ..CommonStyle::default()
                            },
                            ..FlexStyle::default()
                        }),
                    },
                )
                .unwrap();
            }
            drop(dom);
            let snapshot = owner.checkpoint().unwrap().unwrap();
            let started = Instant::now();
            black_box(computed.compute_layout(&snapshot, available_space()));
            started.elapsed()
        });
    }
}

fn scene_construction_benchmarks() {
    for size in TREE_SIZES {
        let (shared, _owner, _nodes) = build_tree(size);
        let snapshot = shared.load();
        let mut computed = ComputedState::new();
        computed.compute_layout(&snapshot, available_space());
        let mut scene = SceneState::new();
        measure(
            "vello_scene_construction",
            size,
            iterations_for(size),
            || {
                scene
                    .rebuild(
                        black_box(snapshot.as_ref()),
                        black_box(&computed),
                        PhysicalSize::new(1_280, 720),
                        Size {
                            width: VIEWPORT_WIDTH,
                            height: VIEWPORT_HEIGHT,
                        },
                        1.0,
                    )
                    .unwrap();
                black_box(&scene);
                scene.source_revision()
            },
        );
    }
}

fn queue_load_benchmarks() {
    const CAPACITY: usize = 1_024;
    const SUBMISSIONS: usize = CAPACITY * 4;
    const ITERATIONS: u64 = 100;

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    for (name, role) in [
        ("mts_queue_under_load", RuntimeRole::Main),
        ("bts_queue_under_load", RuntimeRole::Background),
    ] {
        let runtime = tokio_runtime
            .block_on(
                Runtime::builder()
                    .role(role)
                    .macrotask_capacity(CAPACITY)
                    .build(),
            )
            .unwrap();
        let queue = runtime.macrotask_queue();
        let mut last_dropped = 0_u64;
        let mut observed_high_water = 0_usize;
        measure(name, SUBMISSIONS, ITERATIONS, || {
            let mut dropped = 0_u64;
            let mut high_water = 0_usize;
            for _ in 0..SUBMISSIONS {
                if matches!(
                    queue.try_enqueue(|_| Ok(())),
                    Err(MacrotaskQueueError::Full)
                ) {
                    dropped += 1;
                }
                high_water = high_water.max(queue.depth());
            }
            last_dropped = dropped;
            observed_high_water = observed_high_water.max(high_water);
            (dropped, high_water)
        });
        eprintln!(
            "{name}: last batch dropped {last_dropped}, queue high-water {observed_high_water}/{CAPACITY}"
        );
        tokio_runtime.block_on(runtime.shutdown()).unwrap();
    }
}
