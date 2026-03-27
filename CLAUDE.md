# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a **Hyper.Video Browser Client Simulator**: a Rust TUI that spawns and controls Chromium-backed browser participants for manual session testing.

The active workspace has one shipped binary:
- **client-simulator**: main TUI entrypoint

The core crates are:
- **browser/**: participant automation, shared local runtime, frontend drivers, remote stub
- **config/**: CLI and YAML configuration
- **tui/**: ratatui-based interface

## Build & Development Commands

### Building and Running

```bash
# Build everything (release mode)
cargo build --release

# Run the main TUI simulator
just run              # release mode
just dev              # dev mode (faster compilation)
```

### Testing and Linting

```bash
# Run tests (uses cargo-nextest)
just test
# or manually:
cargo nextest run --no-tests warn

# Lint with clippy (strict - fails on warnings)
just clippy
# or manually:
cargo clippy --all-targets --all-features -- -D warnings

# Auto-format code
just fmt
# or manually:
cargo fmt

# Run both clippy and tests
just check
```

### Nix Support

The project includes Nix flake support for reproducible builds:

```bash
# Build via Nix
nix build .#client-simulator

# Run via Nix
just run-nix
```

### Fetch Session Cookie

```bash
# Get a hyper session cookie for a user
just fetch-cookie [username] [server-url]
# Example:
just fetch-cookie my-user http://localhost:8081
```

## Architecture

### Workspace Structure

```
client-simulator/           # Main binary (TUI)
├── browser/                # Browser automation core
├── config/                 # Configuration management
└── tui/                    # Terminal UI (ratatui-based)
```

### Core Components

#### 1. Browser Module (`browser/`)

The foundation of all simulation modes. Key responsibilities:

- **Browser Lifecycle**: Launches headless/headed Chromium instances using `chromiumoxide`
- **Participant**: Central abstraction representing a simulated user
  - `frontend.rs`: shared local Chromium runtime shell
  - `ParticipantInner`: hyper core frontend driver
  - `ParticipantInnerLite`: hyper-lite frontend driver
  - `remote_stub.rs`: In-process placeholder for the future remote backend
- **Authentication**: `HyperSessionCookieStash` manages persistent user sessions
- **Media Handling**: Supports fake media sources (builtin, custom video/audio files)

Key files:
- `browser/src/participant/mod.rs`: Participant API and lifecycle
- `browser/src/participant/frontend.rs`: Shared runtime and frontend resolution
- `browser/src/participant/inner.rs`: Hyper core frontend driver
- `browser/src/participant/inner_lite.rs`: Hyper-lite frontend driver
- `browser/src/participant/remote_stub.rs`: Endpoint-free remote participant stub
- `browser/src/auth.rs`: Cookie/session management

#### 2. Config Module (`config/`)

Unified configuration system supporting CLI args, YAML files, and environment variables:

- `Config`: Main config struct with media settings, transport modes, etc.
- `ParticipantBackendKind`: Explicit local vs remote-stub backend selection
- `ParticipantConfig`: Per-participant settings (username, audio/video, resolution)
- `BrowserConfig`: Browser-specific settings (user data dir, headless mode)

#### 3. TUI Mode (`tui/` + main binary)

Interactive terminal interface built with `ratatui`:
- Spawn/control participants manually
- Pick the participant backend explicitly
- Toggle audio/video/screenshare
- View logs in real-time
- Persist configuration across sessions

### Participant State Machine

Participants follow this lifecycle:

1. **Spawned**: Browser launched, navigating to session URL
2. **Authenticated**: Cookie set, ready to join
3. **Joined**: Connected to space, media negotiation complete
4. **Active**: Can toggle audio/video/screenshare, send commands
5. **Closed**: Browser terminated

### Browser Automation Strategy

The simulator uses two approaches:

1. **Full Browser** (`ParticipantInner`):
   - Launches real Chromium via CDP (Chrome DevTools Protocol)
   - Uses the shared local runtime plus the hyper core driver
   - Supports all features (background blur, noise suppression, etc.)
   - CSS selectors in `browser/src/participant/selectors.rs`

2. **Lite Mode** (`ParticipantInnerLite`):
   - Uses the same shared runtime with a hyper-lite-specific driver
   - Keeps the browser-based participant model
   - Supports a smaller command surface than hyper core

### Cookie/Session Management

- Persistent cookies stored in data directory (`~/.local/share/client-simulator/`)
- `HyperSessionCookieStash`: File-based cookie storage
- `HyperSessionCookieManger`: In-memory cookie manager (Arc-wrapped for sharing)
- Supports fetching new cookies via login flow

### Media Configuration

Three fake media modes:

1. **None**: Use real webcam/mic (requires hardware)
2. **Builtin**: Chromium's built-in fake media (`--use-fake-device-for-media-stream`)
3. **File/URL**: Custom video/audio files (`--use-file-for-fake-video-capture=...`)

Custom media files are cached in `~/.cache/client-simulator/`.

## Common Development Tasks

### Running a Single Test

```bash
# Run specific test by name
cargo nextest run test_name

# Run tests in specific package
cargo nextest run -p client-simulator-browser
```

### Adding a New Participant Command

1. Add variant to `ParticipantMessage` enum in `browser/src/participant/messages.rs`
2. Handle it in the shared runtime/driver boundary (`browser/src/participant/frontend.rs`)
3. Implement frontend-specific behavior in `browser/src/participant/inner.rs` and/or `browser/src/participant/inner_lite.rs`
4. Add the public method to `Participant` in `browser/src/participant/mod.rs`
5. Expose it in the TUI if needed

### Debugging Browser Issues

- Set `headless: false` in config to see browser window
- Enable verbose logging: `RUST_LOG=debug cargo run`
- Check browser console via DevTools (when headless=false)
- Use `--debug` flag: `just dev --debug`

### Working with Chromiumoxide

The project uses a forked version (`caido/dependency-chromiumoxide`) due to JSON parsing issues. See `Cargo.toml` workspace dependencies for details.

Key chromiumoxide patterns:
```rust
// Navigate to page
page.goto(url).await?;

// Wait for element
let element = wait_for_element(&page, "button.join", Duration::from_secs(10)).await?;

// Click element
element.click().await?;

// Evaluate JavaScript
page.evaluate("document.querySelector('video')").await?;
```

## Project-Specific Notes

### Clippy Lints

The project allows specific lints (see `Cargo.toml`):
- `module_inception`: Allows simpler exports
- `type_complexity`: Complex types (e.g., `Rc<RefCell<T>>`) are semantically important
- `too_many_arguments`: Case-by-case basis

### Transport Modes

Participants can use different WebRTC transport modes:
- **webtransport**
- **webrtc**
- Configured via `TransportMode` enum

### Noise Suppression & Resolution

- Noise suppression is configured via the `NoiseSuppression` enum
- Webcam resolutions are configured via the `WebcamResolution` enum

## Important File Locations

- Main config: `~/.config/client-simulator/config.yaml`
- User data (cookies): `~/.local/share/client-simulator/`
- Media cache: `~/.cache/client-simulator/`
- Browser user data dirs: Created per-participant in temp directories
