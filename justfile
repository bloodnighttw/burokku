default:
    @just --list

build:
    cargo build --workspace

check:
    cargo check --workspace
    env CI=true pnpm --filter @burokku/ui typecheck
    env CI=true pnpm --filter @burokku/example-react typecheck

test:
    cargo test --workspace
    env CI=true pnpm --filter @burokku/ui typecheck
    env CI=true pnpm --filter @burokku/ui test
    env CI=true pnpm --filter @burokku/example-react typecheck
    env CI=true pnpm --filter @burokku/example-react build
    cargo run -p burokku -- --check-ui example/react/dist/app.js

run *args:
    cargo run -p burokku -- {{args}}
