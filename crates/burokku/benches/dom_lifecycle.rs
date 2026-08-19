use std::{hint::black_box, time::Duration, time::Instant};

use runtime::{Runtime, RuntimeRole};

const TREE_SIZES: [usize; 3] = [100, 1_000, 10_000];

fn main() {
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    println!("benchmark\ttree_size\titerations\ttotal_ms\tus_per_iteration");
    for size in TREE_SIZES {
        tokio.block_on(benchmark_tree(size));
    }
}

async fn benchmark_tree(size: usize) {
    let (dom_plugin, _shared) = burokku::ui::js_bridge::DomPlugin::with_new_dom();
    let runtime = Runtime::builder()
        .role(RuntimeRole::Background)
        .plugin(dom_plugin)
        .build()
        .await
        .unwrap();

    runtime
        .eval::<()>(format!(
            r#"
            globalThis.benchNodes = [];
            for (let index = 0; index < {size}; index++) {{
              const node = document.createElement("div");
              document.body.appendChild(node);
              benchNodes.push(node);
            }}
            globalThis.benchGeneration = 0;
            "#,
        ))
        .await
        .unwrap();
    checkpoint_barrier(&runtime).await;

    let iterations = iterations_for(size);
    measure_eval(
        &runtime,
        "clean_checkpoint",
        size,
        iterations,
        "void benchNodes.length",
    )
    .await;
    measure_eval(
        &runtime,
        "attribute_checkpoint",
        size,
        iterations,
        r#"
        benchGeneration++;
        benchNodes[benchGeneration % benchNodes.length]
          .setAttribute("data-generation", benchGeneration);
        "#,
    )
    .await;

    runtime.shutdown().await.unwrap();
}

async fn measure_eval(
    runtime: &Runtime,
    name: &str,
    size: usize,
    iterations: u64,
    source: &'static str,
) {
    runtime.eval::<()>(source).await.unwrap();
    checkpoint_barrier(runtime).await;

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(runtime.eval::<()>(source).await.unwrap());
    }
    checkpoint_barrier(runtime).await;
    report(name, size, iterations, started.elapsed());
}

async fn checkpoint_barrier(runtime: &Runtime) {
    // Runtime::eval returns from inside its macrotask. Starting a following
    // macrotask guarantees that pending jobs and plugin checkpoints for the
    // preceding operation have completed.
    runtime.eval::<()>("void 0").await.unwrap();
}

fn iterations_for(size: usize) -> u64 {
    match size {
        0..=100 => 2_000,
        101..=1_000 => 500,
        _ => 100,
    }
}

fn report(name: &str, size: usize, iterations: u64, elapsed: Duration) {
    let us_per_iteration = elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64;
    println!(
        "{name}\t{size}\t{iterations}\t{:.3}\t{us_per_iteration:.2}",
        elapsed.as_secs_f64() * 1_000.0,
    );
}
