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

## Run with Nix

Run the simulator directly from the repository without cloning it:

```sh
nix run 'git+https://github.com/hypervideo/browser-simulator.git?submodules=1' -- tui
```

Set `submodules=1` because the build uses files from a Git submodule. The shorter
`github:hypervideo/browser-simulator` flake reference does not fetch submodules.

## Cloudflare worker commands

The `cf` subcommand inspects and closes sessions on the Cloudflare browser
simulator worker:

```sh
hyper-client-simulator cf sessions              # open sessions and a summary
hyper-client-simulator cf limits                # Browser Rendering limits
hyper-client-simulator cf close <SESSION_IDS>   # one ID, a comma-separated list, or `all`
```

The commands read the worker URL and HTTP timeout from `cloudflare.base_url`
and `cloudflare.request_timeout_seconds` in `config.yaml`. Override them per
call with `--base-url <URL>`, `--local` (use `http://127.0.0.1:8787`), and
`--timeout <SECONDS>`. Add `--json` to print the raw worker response.

`hyper-client-simulator aws` offers the same for AWS Device Farm sessions. Run
`hyper-client-simulator aws --help` for details.

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
