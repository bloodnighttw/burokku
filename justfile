default:
    @just --list

build:
    pnpm typecheck
    pnpm --filter './example/*' build
    cargo build --workspace

check:
    env CI=true pnpm typecheck
    env CI=true pnpm --filter './example/*' build
    cargo check --workspace

test:
    cargo test --workspace

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
