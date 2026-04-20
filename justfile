default:
    just --list

# -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

run *flags="":
    cargo run --release --package hyper-client-simulator -- {{ flags }}

dev *flags="":
    cargo run --package hyper-client-simulator -- {{ flags }}

run-nix *flags="":
    nix run .#hyper-client-simulator -- {{ flags }}

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

dist-init *args="":
    dist init {{ args }}

dist-generate *args="":
    dist generate {{ args }}

dist-plan *args="":
    dist plan {{ args }}

dist-build *args="":
    dist build {{ args }}

# -=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-

fetch-cookie username="simulator-user" server-url="http://localhost:8081":
    cargo run -q -- cookie --url {{ server-url }} --user {{ username }}

cachix-push:
    nix build --no-link --print-out-paths \
        .#hyper-client-simulator \
      | cachix push hyper-video
