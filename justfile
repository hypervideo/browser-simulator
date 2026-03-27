default:
    just --list

# -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

run *flags="":
    cargo run --release --package client-simulator -- {{ flags }}

dev *flags="":
    cargo run --package client-simulator -- {{ flags }}

run-nix *flags="":
    nix run .#client-simulator -- {{ flags }}

# -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

stats-gatherer *flags="":
    cargo run --release --bin client-simulator-stats-gatherer -- {{ flags }}

stats-gatherer-dev *flags="":
    cargo run --package client-simulator-stats-gatherer -- {{ flags }}

stats-gatherer-nix *flags="":
    nix run .#client-simulator-stats-gatherer -- {{ flags }}

# -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

clippy-watch:
    fd --type f --extension rs | entr -n -r just clippy

test:
    cargo nextest run --no-tests warn

check: clippy test

fmt:
    cargo fmt

# -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

fetch-cookie username="simulator-user" server-url="http://localhost:8081":
    cargo run -q -- cookie --url {{ server-url }} --user {{ username }}

cachix-push:
    nix build --no-link --print-out-paths \
        .#client-simulator \
        .#client-simulator-stats-gatherer \
      | cachix push hyper-video
