default:
    @just --list

build:
    cargo build --workspace
    pnpm --filter './example/*' build

check:
    cargo check --workspace
    env CI=true pnpm --filter './example/*' typecheck

test:
    cargo test --workspace
    env CI=true pnpm --filter @burokku/example-counter check
    env CI=true pnpm --filter @burokku/example-layouts check

counter:
    pnpm --filter @burokku/example-counter dev

layouts:
    pnpm --filter @burokku/example-layouts dev

profile-counter:
    pnpm --filter @burokku/example-counter build
    env CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release -p burokku-example-counter
    samply record --profile-name "burokku counter" -- target/release/burokku-example-counter

profile-layouts:
    pnpm --filter @burokku/example-layouts build
    env CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release -p burokku-example-layouts
    samply record --profile-name "burokku layouts" -- target/release/burokku-example-layouts

run *args:
    cargo run -p burokku -- {{args}}
