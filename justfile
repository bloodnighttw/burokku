default:
    @just --list

build:
    cargo build --workspace

check:
    cargo check --workspace
    env CI=true pnpm --filter @burokku/runtime typecheck
    env CI=true pnpm --filter @burokku/react typecheck
    env CI=true pnpm --filter @burokku/solid typecheck
    env CI=true pnpm --filter @burokku/example-react typecheck
    env CI=true pnpm --filter @burokku/example-solid typecheck

test:
    cargo test --workspace
    env CI=true pnpm --filter @burokku/runtime typecheck
    env CI=true pnpm --filter @burokku/react typecheck
    env CI=true pnpm --filter @burokku/solid typecheck
    env CI=true pnpm --filter @burokku/example-react typecheck
    env CI=true pnpm --filter @burokku/example-react build
    cargo run -p burokku -- --check-dom example/react/dist/app.js
    env CI=true pnpm --filter @burokku/example-solid typecheck
    env CI=true pnpm --filter @burokku/example-solid build
    cargo run -p burokku -- --check-dom example/solid/dist/app.js

react:
    pnpm --filter @burokku/example-react build
    cargo run -p burokku -- example/react/dist/app.js

solid:
    pnpm --filter @burokku/example-solid build
    cargo run -p burokku -- example/solid/dist/app.js

run *args:
    cargo run -p burokku -- {{args}}
