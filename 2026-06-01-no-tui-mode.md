# Headless Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `hyper-client-simulator headless` so simulator participants can be started from CLI arguments without entering the TUI or saving runtime options back to the app config.

**Architecture:** Keep headless behavior local to the root binary. Load the existing `Config` once through the current config path, apply typed CLI overrides in memory, clone that config per participant, apply each participant JSON override in memory, spawn participants through `ParticipantStore`, and handle shutdown with a small `tokio::select!` loop.

**Tech Stack:** Rust 2021, `clap`, `serde`, `serde_json`, `tokio`, `tracing-subscriber`, existing `client-simulator-config` and `client-simulator-browser` crates.

---

## Scope

Initial participant JSON supports stable launch fields:

- `url`
- `backend`
- `headless`
- `audio_enabled`
- `video_enabled`
- `screenshare_enabled`
- `auto_gain_control`
- `noise_suppression`
- `transport`
- `resolution`
- `blur`

Nested backend service configs such as `cloudflare` and `device_farm` stay out of the initial implementation. Existing app config/global defaults still provide those values. Add nested per-participant backend overrides later only if a concrete workflow needs them.

If no `--participant` is supplied, start one participant using the global effective config. If one or more `--participant` values are supplied, start exactly one participant per JSON object.

---

### Task 1: Add Headless CLI Types and Override Logic

**Files:**
- Modify: `Cargo.toml`
- Create: `src/headless.rs`

1. Add root-crate dependencies if missing:

```toml
serde.workspace = true
```

2. Create `src/headless.rs` with headless args, participant JSON overrides, and direct apply helpers.

Use typed fields. Prefer clap parsers that accept the intended UX:

```rust
#[derive(clap::Args, Debug, Clone, Default)]
pub struct HeadlessArgs {
    #[clap(long, value_name = "URL")]
    pub url: Option<url::Url>,

    #[clap(long, value_name = "BACKEND")]
    pub backend: Option<client_simulator_config::ParticipantBackendKind>,

    #[clap(long = "headless", value_parser = clap::builder::BoolishValueParser::new())]
    pub headless: Option<bool>,

    #[clap(long = "audio-enabled", value_parser = clap::builder::BoolishValueParser::new())]
    pub audio_enabled: Option<bool>,

    #[clap(long = "video-enabled", value_parser = clap::builder::BoolishValueParser::new())]
    pub video_enabled: Option<bool>,

    #[clap(long = "screenshare-enabled", value_parser = clap::builder::BoolishValueParser::new())]
    pub screenshare_enabled: Option<bool>,

    #[clap(long = "auto-gain-control", value_parser = clap::builder::BoolishValueParser::new())]
    pub auto_gain_control: Option<bool>,

    #[clap(long = "noise-suppression", value_name = "MODEL")]
    pub noise_suppression: Option<client_simulator_config::NoiseSuppression>,

    #[clap(long, value_name = "MODE")]
    pub transport: Option<client_simulator_config::TransportMode>,

    #[clap(long, value_name = "RESOLUTION")]
    pub resolution: Option<client_simulator_config::WebcamResolution>,

    #[clap(long, value_parser = clap::builder::BoolishValueParser::new())]
    pub blur: Option<bool>,

    #[clap(long = "participant", value_name = "JSON")]
    pub participants: Vec<String>,
}
```

If one of the config enums does not satisfy clap's `FromStr` bounds cleanly, keep that field as `Option<String>` and parse it with the enum's existing `FromStr` implementation inside `apply_cli_overrides`, returning an `eyre` error with the flag name.

3. Add the participant override type:

```rust
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ParticipantOverride {
    url: Option<url::Url>,
    backend: Option<client_simulator_config::ParticipantBackendKind>,
    headless: Option<bool>,
    audio_enabled: Option<bool>,
    video_enabled: Option<bool>,
    screenshare_enabled: Option<bool>,
    auto_gain_control: Option<bool>,
    noise_suppression: Option<client_simulator_config::NoiseSuppression>,
    transport: Option<client_simulator_config::TransportMode>,
    resolution: Option<client_simulator_config::WebcamResolution>,
    blur: Option<bool>,
}
```

4. Add direct override helpers:

```rust
fn apply_cli_overrides(config: &mut client_simulator_config::Config, args: &HeadlessArgs) {
    if let Some(url) = &args.url {
        config.url = Some(url.clone());
    }
    if let Some(backend) = args.backend {
        config.backend = backend;
    }
    if let Some(headless) = args.headless {
        config.headless = headless;
    }
    if let Some(audio_enabled) = args.audio_enabled {
        config.audio_enabled = audio_enabled;
    }
    if let Some(video_enabled) = args.video_enabled {
        config.video_enabled = video_enabled;
    }
    if let Some(screenshare_enabled) = args.screenshare_enabled {
        config.screenshare_enabled = screenshare_enabled;
    }
    if let Some(auto_gain_control) = args.auto_gain_control {
        config.auto_gain_control = auto_gain_control;
    }
    if let Some(noise_suppression) = args.noise_suppression {
        config.noise_suppression = noise_suppression;
    }
    if let Some(transport) = args.transport {
        config.transport = transport;
    }
    if let Some(resolution) = args.resolution {
        config.resolution = resolution;
    }
    if let Some(blur) = args.blur {
        config.blur = blur;
    }
}

fn apply_participant_override(
    mut config: client_simulator_config::Config,
    override_: ParticipantOverride,
) -> client_simulator_config::Config {
    if let Some(url) = override_.url {
        config.url = Some(url);
    }
    if let Some(backend) = override_.backend {
        config.backend = backend;
    }
    if let Some(headless) = override_.headless {
        config.headless = headless;
    }
    if let Some(audio_enabled) = override_.audio_enabled {
        config.audio_enabled = audio_enabled;
    }
    if let Some(video_enabled) = override_.video_enabled {
        config.video_enabled = video_enabled;
    }
    if let Some(screenshare_enabled) = override_.screenshare_enabled {
        config.screenshare_enabled = screenshare_enabled;
    }
    if let Some(auto_gain_control) = override_.auto_gain_control {
        config.auto_gain_control = auto_gain_control;
    }
    if let Some(noise_suppression) = override_.noise_suppression {
        config.noise_suppression = noise_suppression;
    }
    if let Some(transport) = override_.transport {
        config.transport = transport;
    }
    if let Some(resolution) = override_.resolution {
        config.resolution = resolution;
    }
    if let Some(blur) = override_.blur {
        config.blur = blur;
    }
    config
}
```

5. Add focused tests in `src/headless.rs`:

- CLI parsing accepts the user-shaped command with repeated `--participant`.
- CLI bool options accept explicit values, for example `--headless false`.
- participant JSON overrides global config.
- unknown participant JSON field returns an error.
- empty participant list returns one effective config.

Run:

```bash
cargo test -p hyper-client-simulator headless
```

---

### Task 2: Implement the Headless Runner

**Files:**
- Modify: `src/headless.rs`

1. Add `run(args, debug) -> eyre::Result<i32>`.

Implementation shape:

```rust
pub async fn run(args: HeadlessArgs, debug: u8) -> eyre::Result<i32> {
    init_logging(debug)?;

    let mut global_config = client_simulator_config::Config::new(client_simulator_config::TuiArgs::default())?;
    apply_cli_overrides(&mut global_config, &args);

    let participant_configs = build_participant_configs(global_config.clone(), &args.participants)?;
    let store = client_simulator_browser::participant::ParticipantStore::new(global_config.data_dir());

    for config in &participant_configs {
        store.spawn(config)?;
    }

    Ok(wait_for_exit(store).await)
}
```

This uses the existing config/default loading path but never calls `Config::save()` or `Config::update_from_args()`.

2. Add `build_participant_configs`:

```rust
fn build_participant_configs(
    global_config: client_simulator_config::Config,
    participants: &[String],
) -> eyre::Result<Vec<client_simulator_config::Config>> {
    if participants.is_empty() {
        return Ok(vec![global_config]);
    }

    participants
        .iter()
        .map(|json| {
            let override_: ParticipantOverride = serde_json::from_str(json)?;
            Ok(apply_participant_override(global_config.clone(), override_))
        })
        .collect()
}
```

Wrap JSON errors with context containing `--participant` so invalid input is easy to diagnose.

3. Add logging with normal `tracing_subscriber`, not `tui_logger`:

```rust
fn init_logging(debug: u8) -> eyre::Result<()> {
    use tracing_subscriber::{
        fmt::{
            self,
            writer::MakeWriterExt as _,
        },
        prelude::*,
        EnvFilter,
    };

    let filter = match debug {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let writer = std::io::stderr
        .with_min_level(tracing::Level::WARN)
        .or_else(std::io::stdout);

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(fmt::layer().with_writer(writer).with_filter(EnvFilter::builder().parse_lossy(filter)))
        .try_init()?;

    Ok(())
}
```

4. Add shutdown behavior:

```rust
async fn wait_for_exit(store: client_simulator_browser::participant::ParticipantStore) -> i32 {
    tokio::select! {
        _ = wait_for_all_stopped(&store) => 0,
        _ = wait_for_ctrl_c() => {
            tracing::info!("Shutting down participants. Press Ctrl-C again to force exit.");
            tokio::select! {
                _ = store.shutdown_all() => 0,
                _ = wait_for_ctrl_c() => 130,
            }
        }
    }
}

async fn wait_for_ctrl_c() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!("Failed to listen for Ctrl-C: {err}");
    }
}

async fn wait_for_all_stopped(store: &client_simulator_browser::participant::ParticipantStore) {
    let mut states = store
        .values()
        .into_iter()
        .map(|participant| participant.state.clone())
        .collect::<Vec<_>>();

    for state in &mut states {
        let _ = state.wait_for(|current| !current.running).await;
    }
}
```

5. Add a small pure shutdown-coordination test if desired. Do not spawn local Chromium in unit tests. Use injected futures or channels to verify "first signal starts shutdown" and "second signal returns 130".

Run:

```bash
cargo test -p hyper-client-simulator headless
```

---

### Task 3: Wire the Command in `main`

**Files:**
- Modify: `src/main.rs`

1. Add the module:

```rust
mod headless;
```

2. Add the subcommand:

```rust
#[derive(Subcommand, Debug)]
enum Command {
    /// Start the TUI application
    Tui(TuiArgs),
    /// Start simulator participants without the TUI
    Headless(headless::HeadlessArgs),
    /// Connect to the hyper server to get a hyper session cookie
    Cookie(CookieArgs),
}
```

3. Route the command:

```rust
Some(Command::Headless(args)) => {
    let code = headless::run(args, debug).await?;
    std::process::exit(code);
}
```

4. Add or keep a CLI parsing test proving the example shape works:

```rust
#[test]
fn parses_headless_command_with_repeated_participants() {
    let args = CliArgs::parse_from([
        "hyper-client-simulator",
        "--debug",
        "headless",
        "--url",
        "https://latest.dev.hyper.video/F27-T5F-DXY",
        "--participant",
        r#"{"audio_enabled": false, "backend": "local"}"#,
        "--participant",
        r#"{"audio_enabled": true, "backend": "cloudflare"}"#,
    ]);

    match args.command {
        Some(Command::Headless(headless)) => {
            assert_eq!(args.debug, 1);
            assert_eq!(headless.url.as_ref().map(url::Url::as_str), Some("https://latest.dev.hyper.video/F27-T5F-DXY"));
            assert_eq!(headless.participants.len(), 2);
        }
        other => panic!("expected headless command, got {other:?}"),
    }
}
```

Run:

```bash
cargo test -p hyper-client-simulator parses_headless_command_with_repeated_participants
```

---

### Task 4: Verify End to End

**Files:**
- No new files.

Run formatting, linting, and tests:

```bash
just fmt
just clippy
just test
```

Manual smoke test with a backend that does not require Chromium:

```bash
cargo run -- headless \
  --url "https://example.com/lite/demo" \
  --participant '{"backend": "remote-stub", "audio_enabled": false}'
```

Expected:

- logs print in the terminal without entering TUI mode
- no config file is written from CLI overrides
- first `Ctrl-C` starts participant shutdown and exits cleanly
- second `Ctrl-C` exits with code `130`

Manual example matching the requested command shape:

```bash
cargo run -- headless \
  --url "https://latest.dev.hyper.video/F27-T5F-DXY" \
  --participant '{"audio_enabled": false, "backend": "local"}' \
  --participant '{"audio_enabled": true, "backend": "cloudflare"}'
```

Expected:

- two participants start
- each participant receives the global URL
- each participant JSON overrides its own audio/backend settings
- logs stream to stdout/stderr through tracing

---

## Non-Goals

- No TUI changes.
- No config crate changes unless implementation proves a small `Config::load()` helper is necessary.
- No config persistence from `headless`.
- No nested per-participant `cloudflare` or `device_farm` overrides in the first version.
- No broad refactor of participant spawning or logging.
