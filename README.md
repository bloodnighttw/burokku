# Burokku

A pnpm monorepo for a Rust library and its TypeScript binding.

## Layout

- `crates/burokku` — Rust source compiled to WebAssembly with `wasm-bindgen`
- `packages/binding` — TypeScript API built with `tsdown`
- `example` — runnable TypeScript consumer

## Getting started

```sh
pnpm install
pnpm build
pnpm dev
```

The example prints a greeting and the result of an addition implemented in Rust.

Run checks with:

```sh
pnpm test
```
