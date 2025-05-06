default:
    just --list

run:
    cargo run

udeps:
    CARGO_TARGET_DIR=target-udeps nix develop .#nightly --command cargo udeps
