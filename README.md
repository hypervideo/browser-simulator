This is a **Hyper.Video Browser Client Simulator**: a Rust TUI for spawning and controlling Chromium-backed browser participants against Hyper.Video sessions.

The active workspace is centered on:
- **client-simulator**: the main TUI binary
- **browser/**: participant automation and remote stub support
- **config/**: CLI and YAML configuration
- **tui/**: terminal UI components

## Install

Tagged macOS releases are published as Homebrew formulae.

```sh
brew tap hypervideo/tap
brew install client-simulator
```

To upgrade later:

```sh
brew upgrade client-simulator
```

The Homebrew package installs the simulator binary only. Chrome or Chromium must
still be installed locally on the Mac where you run it.
