default:
    just --list

run *flags="":
    cargo run -- {{ flags}}

udeps:
    CARGO_TARGET_DIR=target-udeps nix develop .#nightly --command cargo udeps
