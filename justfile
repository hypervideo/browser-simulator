default:
    just --list

run *flags="":
    cargo run --release --package client-simulator-tui -- {{ flags }}

dev *flags="":
    cargo run --package client-simulator-tui -- {{ flags }}

serve *flags="":
    cargo run --release --bin client-simulator-http -- {{ flags }}

serve-dev *flags="":
    cargo run --package client-simulator-http -- {{ flags }}

clippy:
    cargo clippy --all-features -- -D warnings

clippy-watch:
    fd --type f --extension rs | entr -r just clippy
