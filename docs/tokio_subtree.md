# Maintaining the Tokio Git subtree

The complete `tokio-rs/tokio` repository is imported, without squashing, under
`crates/tokio`. The actual patched crate is `crates/tokio/tokio`, selected by
the root workspace's `[patch.crates-io]` entry.

The initial subtree base is the `tokio-1.53.1` tag. Because the import is not
squashed, upstream commits remain connected to this repository's history.

## Remotes

A Git remote is local configuration and is not cloned with the repository. Add
the official remote after cloning:

```sh
git remote add tokio-upstream https://github.com/tokio-rs/tokio.git
git fetch tokio-upstream --tags
```

After creating a GitHub fork, add it separately:

```sh
git remote add tokio-fork git@github.com:YOUR_ORG/tokio.git
git fetch tokio-fork
```

## Publish the subtree to the fork

Push only the subtree history to a dedicated branch in the fork:

```sh
git subtree push \
  --prefix=crates/tokio \
  tokio-fork \
  burokku-external-runtime
```

The Burokku repository remains a single monorepo; the fork is the shareable
upstream-history remote for the extracted Tokio subtree.

## Synchronize a future Tokio release

First fetch the desired official release, then pull it into the subtree without
`--squash`:

```sh
git fetch tokio-upstream --tags

git subtree pull \
  --prefix=crates/tokio \
  tokio-upstream \
  tokio-NEW_VERSION \
  -m "vendor: sync Tokio to NEW_VERSION"
```

Resolve conflicts only in `crates/tokio`, then rerun the validation documented
in `docs/tokio_external_event_loop.md`. In particular, review current-thread
scheduler, composite-driver, Mio registration lifetime, signal/process, timer,
and LocalSet changes before accepting the sync.

After validation, publish the updated subtree branch:

```sh
git subtree push \
  --prefix=crates/tokio \
  tokio-fork \
  burokku-external-runtime
```

## Patch locations

The downstream Tokio implementation is concentrated in:

```text
crates/tokio/tokio/src/runtime/external.rs
crates/tokio/tokio/src/runtime/builder.rs
crates/tokio/tokio/src/runtime/config.rs
crates/tokio/tokio/src/runtime/driver.rs
crates/tokio/tokio/src/runtime/runtime.rs
crates/tokio/tokio/src/runtime/local_runtime/runtime.rs
crates/tokio/tokio/src/runtime/scheduler/current_thread/mod.rs
crates/tokio/tokio/src/runtime/io/driver.rs
crates/tokio/tokio/src/runtime/io/driver/signal.rs
crates/tokio/tokio/src/runtime/time/mod.rs
crates/tokio/tokio/src/task/local.rs
crates/tokio/tokio/tests/rt_external_driver.rs
```

Platform integration and compatibility tests remain owned by Burokku outside
the subtree.
