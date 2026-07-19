default:
    @just --list

build:
    cargo build --workspace

check:
    cargo check --workspace

test:
    cargo test --workspace

run *args:
    cargo run -p burokku -- {{args}}
