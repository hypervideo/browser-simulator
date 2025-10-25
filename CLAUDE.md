# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a **Hyper.Video Browser Client Simulator** - a Rust-based testing framework that simulates multiple browser clients connecting to Hyper.Video sessions. It automates browser interactions using Chromium via the `chromiumoxide` library to test real-time video conferencing functionality at scale.

The project is a Cargo workspace with multiple binaries for different use cases:
- **client-simulator** (main TUI): Interactive terminal UI for manual testing
- **client-simulator-http**: HTTP/WebSocket server for remote control
- **client-simulator-orchestrator**: Batch orchestration of multiple simulated clients
- **client-simulator-stats-gatherer**: Analytics collection from ClickHouse

## Build & Development Commands

### Building and Running

```bash
# Build everything (release mode)
cargo build --release

# Run the main TUI simulator
just run              # release mode
just dev              # dev mode (faster compilation)

# Run HTTP server (for remote control)
just serve            # release mode
just serve-dev        # dev mode

# Run orchestrator (batch mode)
just orchestrator --config path/to/config.yaml
just orchestrator-dev --config path/to/config.yaml

# Run stats gatherer
just stats-gatherer --clickhouse-url http://localhost:8123 --space-url https://...
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
nix build .#client-simulator-http
nix build .#client-simulator-orchestrator
nix build .#client-simulator-stats-gatherer

# Run via Nix
just run-nix
just serve-nix
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
├── tui/                    # Terminal UI (ratatui-based)
├── http/                   # HTTP/WebSocket API server
├── orchestrator/           # Batch orchestration
└── stats-gatherer/         # ClickHouse analytics
```

### Core Components

#### 1. Browser Module (`browser/`)

The foundation of all simulation modes. Key responsibilities:

- **Browser Lifecycle**: Launches headless/headed Chromium instances using `chromiumoxide`
- **Participant**: Central abstraction representing a simulated user
  - `ParticipantInner`: Full browser-based participant (uses Chromium DevTools Protocol)
  - `ParticipantInnerLite`: Lightweight participant (direct WebSocket, no browser)
- **Authentication**: `HyperSessionCookieStash` manages persistent user sessions
- **Media Handling**: Supports fake media sources (builtin, custom video/audio files)

Key files:
- `browser/src/participant/mod.rs`: Participant API and lifecycle
- `browser/src/participant/inner.rs`: Full browser implementation
- `browser/src/participant/inner_lite.rs`: Lite WebSocket-only implementation
- `browser/src/auth.rs`: Cookie/session management

#### 2. Config Module (`config/`)

Unified configuration system supporting CLI args, YAML files, and environment variables:

- `Config`: Main config struct with media settings, transport modes, etc.
- `ParticipantConfig`: Per-participant settings (username, audio/video, resolution)
- `BrowserConfig`: Browser-specific settings (user data dir, headless mode)

#### 3. TUI Mode (`tui/` + main binary)

Interactive terminal interface built with `ratatui`:
- Spawn/control participants manually
- Toggle audio/video/screenshare
- View logs in real-time
- Persist configuration across sessions

#### 4. HTTP Server Mode (`http/`)

Exposes participants via REST API and WebSocket:
- Create participants remotely
- Send control commands (join, toggle media, etc.)
- Stream logs over WebSocket
- Useful for CI/CD integration

#### 5. Orchestrator Mode (`orchestrator/`)

Batch mode for large-scale testing:
- YAML-based configuration with participant specs
- Distributes participants across multiple HTTP workers (round-robin)
- Supports participant-specific settings and staggered join times
- Configuration validation before execution

Example orchestrator config structure:
```yaml
session_url: https://hyper.video/space/SPACE_ID
workers:
  - url: http://worker1:8081
  - url: http://worker2:8081
defaults:
  headless: true
  audio_enabled: true
participants_specs:
  - username: "user-1"
    wait_to_join_seconds: 5
```

#### 6. Stats Gatherer (`stats-gatherer/`)

Connects directly to ClickHouse to collect analytics:
- Server-level metrics
- Space-level metrics
- Participant audio/video processing stats
- Exports as formatted tables or JSON

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
   - Executes JavaScript in page context
   - Supports all features (background blur, noise suppression, etc.)
   - CSS selectors in `browser/src/participant/selectors.rs`

2. **Lite Mode** (`ParticipantInnerLite`):
   - Direct WebSocket connection (no browser)
   - Faster, lower resource usage
   - Limited features (no video rendering, blur, etc.)

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
2. Handle in `ParticipantInner::run()` message loop (`browser/src/participant/inner.rs`)
3. Add public method to `Participant` struct in `browser/src/participant/mod.rs`
4. Expose in TUI/HTTP/orchestrator as needed

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
- **UDP**: Standard WebRTC
- **TCP**: Fallback for restrictive networks
- Configured via `TransportMode` enum

### Noise Suppression & Resolution

- Noise suppression levels: `Off`, `Low`, `Medium`, `High`
- Webcam resolutions: Multiple presets from 180p to 1080p
- Configurable per-participant in orchestrator mode

## Important File Locations

- Main config: `~/.config/client-simulator/config.yaml`
- User data (cookies): `~/.local/share/client-simulator/`
- Media cache: `~/.cache/client-simulator/`
- Browser user data dirs: Created per-participant in temp directories
