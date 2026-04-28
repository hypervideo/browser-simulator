---
name: release-hyper-browser-simulator
description: Release and publish new versions of the hyper-browser-simulator repository. Use when Codex is asked how to cut a release, bump versions, publish or verify the Homebrew formula, publish or verify the Nix flake package/Cachix cache, debug the generated cargo-dist release workflow, or explain the current release process for this repo.
---

# Release Hyper Browser Simulator

## Overview

Use this skill to prepare, publish, or verify releases of the Rust workspace in this repository. Homebrew releases are automated by `cargo-dist` and GitHub Actions; Nix is exposed through the repository flake and optionally pushed to Cachix by a manual `just` recipe.

## Sources Of Truth

- Workspace version: `Cargo.toml` under `[workspace.package]`.
- Nix package version: `nix/packages.nix`.
- Cargo lockfile package versions: `Cargo.lock`.
- Release config: `dist-workspace.toml`.
- Generated release workflow: `.github/workflows/release.yml`.
- Local release helpers: `justfile`.
- User install docs: `README.md`.

Before making changes, check `git status --short --branch` and work with any existing dirty files without reverting unrelated user edits.

## Current Mechanics

Homebrew:

- `dist-workspace.toml` enables `installers = ["homebrew"]`.
- `cargo-dist` builds `aarch64-apple-darwin` and `x86_64-apple-darwin`.
- Release artifacts are hosted on GitHub.
- Formulae are pushed to `hypervideo/homebrew-tap`.
- CI needs the `HOMEBREW_TAP_TOKEN` secret on `hypervideo/browser-simulator`.
- Prerelease tags do not publish to Homebrew unless `publish-prereleases` is enabled.

Nix:

- `flake.nix` exposes `.#hyper-client-simulator`, `.#client-simulator`, and `.#default`.
- `nix/packages.nix` builds from the repo source and has its own explicit `version`.
- There is no GitHub Actions Nix publish job in this repo.
- `just cachix-push` manually builds `.#hyper-client-simulator` and pushes the result to the `hyper-video` Cachix cache.
- Tagged source can be used directly with `nix run github:hypervideo/browser-simulator/vX.Y.Z#hyper-client-simulator`.

## Prepare A Release

1. Confirm the latest remote state:

```sh
git fetch --tags origin
git tag --sort=-v:refname | head
gh release list --repo hypervideo/browser-simulator --limit 10
```

2. Choose the next SemVer version, using a `v` prefix only for the git tag.

3. Update all version sources:

- `Cargo.toml`: `[workspace.package].version = "X.Y.Z"`.
- `nix/packages.nix`: `version = "X.Y.Z";`.
- `Cargo.lock`: refresh local package versions after the `Cargo.toml` bump.

4. Validate before tagging:

```sh
just fmt
just check
nix build .#hyper-client-simulator
just dist-plan --tag=vX.Y.Z
```

Use `just dist-generate` after changing `dist-workspace.toml`; avoid hand-editing the generated release workflow unless `cargo-dist` cannot express the required change.

## Publish

Create an annotated tag on the release commit and push it:

```sh
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin HEAD
git push origin vX.Y.Z
```

The tag triggers `.github/workflows/release.yml`. It builds macOS tarballs, checksum files, `source.tar.gz`, `dist-manifest.json`, and a Homebrew formula, then creates the GitHub Release and pushes the formula to the tap.

After the tag is published, optionally push the Nix build to Cachix from the same clean release commit:

```sh
just cachix-push
```

## Verify

Check the workflow and release:

```sh
gh run list --repo hypervideo/browser-simulator --workflow Release --limit 10
gh release view vX.Y.Z --repo hypervideo/browser-simulator
```

Check the Homebrew tap formula:

```sh
gh api repos/hypervideo/homebrew-tap/contents/Formula/hyper-client-simulator.rb --jq '.content' | base64 --decode
```

Expected formula details:

- `version "X.Y.Z"`.
- URLs point at `https://github.com/hypervideo/browser-simulator/releases/download/vX.Y.Z/...`.
- SHA256 values match the release assets.

Check Nix:

```sh
nix run github:hypervideo/browser-simulator/vX.Y.Z#hyper-client-simulator -- --help
nix build github:hypervideo/browser-simulator/vX.Y.Z#hyper-client-simulator
```

Remember that the Homebrew package installs only the simulator binary. Chrome or Chromium must still be installed locally on the Mac where the simulator runs.
