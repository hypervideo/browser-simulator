This is a **Hyper.Video Browser Client Simulator**: a Rust TUI for spawning and controlling Chromium-backed browser participants against Hyper.Video sessions.

The active workspace is centered on:
- **hyper-client-simulator**: the main TUI binary
- **browser/**: participant automation and remote stub support
- **config/**: CLI and YAML configuration
- **tui/**: terminal UI components

## Install

Tagged macOS releases are published as Homebrew formulae.

```sh
brew tap hypervideo/tap
brew install hyper-client-simulator
```

To upgrade later:

```sh
brew upgrade hyper-client-simulator
```

The Homebrew package installs the simulator binary only. Chrome or Chromium must
still be installed locally on the Mac where you run it.

## Development

This repository uses `hypervideo/cloudflare-browser-simulator` as a Git submodule
for the Cloudflare worker OpenAPI schema used by `cloudflare-worker-client`.
Clone with submodules, or initialize them after cloning:

```sh
git clone --recurse-submodules git@github.com:hypervideo/browser-simulator.git
git submodule update --init --recursive
```

To consume a newer worker API, first update and commit the generated OpenAPI
artifact in `hypervideo/cloudflare-browser-simulator`, then update this
repository's submodule pointer and make any matching Rust client changes. Worker
deployment remains owned by the Cloudflare worker repository.

GitHub Actions needs the repository secret
`CLOUDFLARE_BROWSER_SIMULATOR_READ_TOKEN` set to a read-only token that can
fetch both this repository and the private worker submodule. Full lint/test CI is
skipped for untrusted fork pull requests because that secret is not exposed
there.
