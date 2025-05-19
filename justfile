default:
    just --list

run *flags="":
    cargo run --release -- {{ flags }}

dev *flags="":
    cargo run -- {{ flags }}

udeps:
    CARGO_TARGET_DIR=target-udeps nix develop .#nightly --command cargo udeps
