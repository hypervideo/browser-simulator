# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace for a browser-based client simulator. The simulator is used for testing video conferencing applications. In particular it is used for different frontends of the hyper.video conferencing system.

- The root crate in `src/` launches the interactive TUI.
- Workspace crates are split by responsibility
  - `browser/` for Chromium-driven participant automation
  - `config/` for CLI and YAML configuration
  - `tui/` for terminal UI components
  - default settings live in `config/src/default-config.yaml`.

## Build, Test, and Development Commands

Start in the provided Nix devshell so the Rust toolchain, `cargo-nextest`, Chromium, FFmpeg, and native libraries match the expected environment:

- `nix develop` enters the default devshell from `flake.nix`.

Once inside the devshell, prefer `just` recipes over ad hoc commands:

- `just dev` runs the main simulator in debug mode.
- `just run` runs the main simulator in release mode.
- `just fmt` applies `cargo fmt`.
- `just clippy` runs `cargo clippy --all-targets --all-features -- -D warnings`.
- `just test` runs the test suite with `cargo nextest`.
- `just check` runs linting and tests together.

## Testing Guidelines

Use `cargo nextest` via `just test`. Add unit tests close to the code under test with `#[cfg(test)] mod tests`, which is the current pattern in files such as `tui/src/tui/action.rs`. When behavior spans crates, add integration tests in a crate-level `tests/` directory. Prefer a TDD approach when implementing new features. Even when making changes, ensure that you capture the old behavior with tests before modifying it.
