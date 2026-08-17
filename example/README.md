# Mixed Rust + TypeScript examples

Both example directories are self-contained mixed projects:

- `Cargo.toml` and `src/main.rs` define a local Rust binary crate that uses the
  `burokku` library builder API.
- `package.json`, `vite.config.ts`, and `src/main.ts` define a local Vite
  TypeScript project.
- `pnpm dev` builds `dist/app.js` with Vite and then runs that directory's own
  Cargo binary. The binary embeds the bundle with `include_str!` and passes it
  to `Burokku`.

Run either project from the workspace root:

```sh
pnpm --filter @burokku/example-counter dev
pnpm --filter @burokku/example-layouts dev
```

Headless integration checks use the same local binaries:

```sh
pnpm --filter @burokku/example-counter check
pnpm --filter @burokku/example-layouts check
```
