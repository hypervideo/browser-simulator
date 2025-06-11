default:
    just --list

run *flags="":
    cargo run --release -- {{ flags }}

dev *flags="":
    cargo run -- {{ flags }}

clippy:
    cargo clippy --all-features -- -D warnings

clippy-watch:
    fd --type f --extension rs | entr -r just clippy
