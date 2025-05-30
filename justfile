default:
    just --list

run *flags="":
    cargo run --release -- {{ flags }}

dev *flags="":
    cargo run -- {{ flags }}
