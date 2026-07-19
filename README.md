# Burokku

A Rust workspace containing an asynchronous JavaScript runtime and the Burokku application.

## Layout

- `crates/runtime` — rquickjs-based JavaScript runtime with Tokio integration
- `crates/burokku` — application that runs JavaScript through the runtime

## Getting started

```sh
cargo build --workspace
cargo run -p burokku
```

The application prints a greeting and the result of a JavaScript calculation. Pass a
JavaScript file as the first argument to run it instead:

```sh
cargo run -p burokku -- ./script.js
```

Run checks with:

```sh
cargo test --workspace
```

The same workflows are available through `just build`, `just run`, and `just test`.
