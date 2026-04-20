# Plan: Build macOS Binary Distribution

## Goal

Set this repo up to ship prebuilt macOS binaries for colleagues without requiring a local Rust toolchain.

Keep the implementation simple:

- use `dist` (`cargo-dist`) to build release archives and generate the release workflow,
- make local macOS Chrome discovery work outside the Nix shell,
- run the macOS release builds on Blacksmith macOS runners.

This plan is intentionally limited to shipping the `client-simulator` CLI for macOS.
It does not try to redesign the runtime, build a `.app` bundle, or add extra packaging formats unless `dist` gives them to us cheaply.

## Constraints And Assumptions

- The shipped binary still depends on a locally installed Chrome/Chromium runtime.
- The current code only discovers Chrome via `PATH`, which is not enough for normal macOS machines.
- `ffmpeg` is only needed for custom fake-media conversion. That should not block the base distribution plan.
- This repo is already a normal Rust workspace with a root binary package, which fits `dist` well.

## Implementation Steps

### 1. Add `dist` configuration for macOS release artifacts

STATUS: completed

Objective:

- introduce the minimal `dist` config needed to build release archives for:
  - `aarch64-apple-darwin`
  - `x86_64-apple-darwin`

Work:

- install `dist` locally using our nix flake and the `cargo-dist` derivation and initialize the workspace with `dist init`
- commit the generated `Cargo.toml` metadata and any generated workflow/config files
- keep the initial target list macOS-only instead of enabling a larger cross-platform matrix right now
- make sure the root package metadata is present and suitable for generated release artifacts:
  - `description`
  - `homepage` or a repository/homepage value usable by release tooling
- use the generated `profile.dist` instead of inventing custom release profiles

Expected result:

- `dist` can produce release archives for both macOS architectures from this workspace
- the repo has a standard `dist` config that can be regenerated later with `dist init`

Notes:

- `dist` can also manage Homebrew later, but the base GitHub Release artifact flow should land first.
- Avoid custom `dist` scripting unless the generated defaults prove insufficient.

### 2. Add `just` commands for local release workflows

STATUS: completed

Objective:

- make local release-related operations obvious and repeatable

Work:

- add a small set of `justfile` commands, likely:
  - `just dist-init`
  - `just dist-generate`
  - `just dist-plan`
  - `just dist-build`
- keep the commands thin wrappers around `dist`
- prefer names that match the `dist` subcommands directly

Expected result:

- contributors can regenerate config and test release builds locally without remembering `dist` syntax

Notes:

- Do not add a large release DSL to `justfile`
- Do not duplicate CI logic in `just`; keep it as a thin local entrypoint

### 3. Extend Chrome discovery for normal macOS installs

STATUS: completed

Objective:

- make the released binary work on a colleague’s Mac without relying on the Nix shell adding Chrome to `PATH`

Current code:

- `browser/src/participant/local/session.rs` looks up:
  - `chromium`
  - `google-chrome`
  - `google-chrome-stable`
  - `chrome`
- if none are on `PATH`, startup fails

Work:

- keep the existing `PATH` lookup first
- add a macOS fallback that checks common app-bundle executable locations, starting with:
  - `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
- optionally also check:
  - `/Applications/Chromium.app/Contents/MacOS/Chromium`
  - `~/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
- return the first existing executable path
- keep the change scoped to one helper, likely `get_binary()`
- add focused unit coverage for the path-resolution logic if the function can be made testable without filesystem-heavy integration tests

Expected result:

- the binary works on standard macOS setups where Chrome is installed in `/Applications`

Notes:

- Keep this as detection logic only
- Do not add a full browser-install manager
- Do not introduce a macOS-only CLI flag unless the fallback logic turns out to be too brittle

### 4. Generate the GitHub release workflow with `dist`

STATUS: completed

Objective:

- let `dist` own the release workflow rather than hand-writing a separate packaging pipeline

Work:

- use `dist`’s GitHub CI integration so `.github/workflows/release.yml` is generated from config
- review the generated workflow and only make the smallest necessary customizations
- keep the CI model close to upstream `dist` conventions so `dist init` / `dist generate` stay usable later

Expected result:

- release builds are driven by a standard `dist` workflow and tag-based release process

Notes:

- Avoid hand-maintaining a fully custom release workflow if `dist` can express the same thing in config
- If workflow customization is needed, prefer `dist` config knobs over direct YAML edits

### 5. Route macOS targets to Blacksmith macOS runners

STATUS: completed

Objective:

- build macOS release artifacts on Blacksmith using the requested runner class

Work:

- configure `dist` custom runners for the macOS targets so generated release jobs use:
  - `blacksmith-12vcpu-macos-latest`
- keep any non-macOS global/planning jobs on the simplest supported runner unless there is a strong reason to move them too
- confirm the generated workflow still remains mostly `dist`-managed

Expected result:

- macOS artifact jobs run on Blacksmith instead of GitHub-hosted macOS runners

Notes:

- The main customization point should be `dist`’s GitHub custom runner configuration, not a hand-edited workflow matrix
- If `dist` requires a separate global runner setting for plan/host jobs, keep that explicit and minimal

### 6. Validate the end-to-end release path

STATUS: completed

Objective:

- confirm the new flow works before relying on it for colleague distribution

Work:

- run the local `just dist-plan` / `just dist-build` commands
- verify the generated artifacts include the `client-simulator` binary for both macOS targets
- open or inspect the generated release workflow
- if practical, test one dry-run or pre-release tag in GitHub Actions
- verify the binary can find Chrome on a normal macOS install outside the Nix shell

Expected result:

- confidence that a tagged release will produce usable macOS artifacts

### 7. Add Homebrew installer support

STATUS: unimplemented

Objective:

- let colleagues install `client-simulator` with Homebrew instead of manually downloading release archives

Work:

- enable the `homebrew` installer in `dist`
- publish to the dedicated tap `hypervideo/homebrew-tap`
  - this tap does not exist yet, instruct the user how to create it
- configure the required Homebrew metadata in `dist` so generated formulae point at the GitHub Release artifacts for this repo
- regenerate the `dist` release workflow so Homebrew publishing is managed by `dist` rather than a separate hand-written pipeline
- document the expected install flow for users, including:
  - `brew tap ...`
  - `brew install client-simulator`
- verify the generated Homebrew formula installs the macOS binary and that the installed binary still relies on the local Chrome/Chromium runtime as expected

Expected result:

- tagged releases publish both plain GitHub Release archives and a Homebrew formula
- colleagues can install or upgrade with `brew`

Notes:

- Keep Homebrew support scoped to the existing macOS CLI distribution
- Do not broaden this into notarization, `.app` bundling, or extra installer formats
- Prefer `dist`'s native Homebrew support over maintaining a custom formula by hand

## Suggested Order Of Execution

1. Add `dist` config.
2. Add the `just` commands.
3. Fix macOS Chrome discovery.
4. Generate and review the release workflow.
5. Configure Blacksmith runners through `dist`.
6. Validate the local and CI release flow.
7. Add Homebrew installer support once the base release flow is stable.

## Non-Goals For This Pass

- building a signed `.app` bundle
- notarization
- DMG packaging
- automatic Chrome installation
- broad Linux/Windows release support

Homebrew is a follow-up phase after the base GitHub Release artifact flow is stable.

## Documentation References

- dist introduction: https://axodotdev.github.io/cargo-dist/book/introduction.html
- dist install: https://axodotdev.github.io/cargo-dist/book/install.html
- dist simple workspace guide: https://axodotdev.github.io/cargo-dist/book/workspaces/simple-guide.html
- dist config reference, including `github-custom-runners`: https://axodotdev.github.io/cargo-dist/book/reference/config.html
- dist Homebrew installer docs, for later follow-up: https://axodotdev.github.io/cargo-dist/book/installers/homebrew.html
- Blacksmith quickstart and runner-tag mapping: https://docs.blacksmith.sh/introduction/quickstart

## Practical Deliverables

When this plan is implemented, the resulting diff should roughly contain:

- `Cargo.toml` updates for `dist`
- `justfile` release commands
- a small macOS Chrome discovery change in `browser/src/participant/local/session.rs`
- a generated `.github/workflows/release.yml`
- any `dist`-managed config files generated by initialization

If the Homebrew follow-up is implemented too, the resulting diff should also roughly contain:

- `dist` installer config enabling Homebrew
- any Homebrew-specific `dist` metadata needed for the chosen tap
- regenerated `dist` release workflow changes required for Homebrew publishing
- short contributor or user-facing install docs showing the `brew tap` / `brew install` flow
