# Headless Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `hyper-client-simulator headless` so simulator participants can be started directly from CLI arguments without entering the TUI or writing runtime options back to the app config.

**Architecture:** Keep the TUI path unchanged. Add a headless runner in the root binary that loads persisted config plus global defaults, overlays command-line settings, overlays each `--participant` JSON object on top of that, spawns participants through `ParticipantStore`, streams tracing output to standard streams, and coordinates graceful/forced shutdown.

**Tech Stack:** Rust 2021, `clap`, `serde`, `serde_json`, `config`, `tokio`, `tracing-subscriber`, existing `client-simulator-config` and `client-simulator-browser` crates.

---

## Design Decisions

- `headless` means "no TUI". It should not implicitly persist settings. It should respect the existing browser `headless` config value unless `--headless true|false` or a participant JSON `{"headless": true}` overrides it.
- If no `--participant` argument is supplied, start one participant using the global effective config. If one or more `--participant` values are supplied, start exactly one participant per JSON object.
- Participant JSON supports the same launch-time fields exposed as global headless CLI overrides: `url`, `backend`, `headless`, `audio_enabled`, `video_enabled`, `screenshare_enabled`, `auto_gain_control`, `noise_suppression`, `transport`, `resolution`, `blur`, plus nested `cloudflare` and `device_farm`.
- Unknown participant JSON fields fail fast. This prevents silent typos such as `audio_enable`.
- The first `Ctrl-C` calls `ParticipantStore::shutdown_all().await`. A second `Ctrl-C` returns a forced-exit result to `main`, which exits with code `130`.
- Logging uses a normal `tracing_subscriber` fmt subscriber, not `tui_logger`. Warnings and errors go to stderr; info/debug/trace go to stdout.

## File Structure

- Modify `Cargo.toml`: add root-crate dependency on `serde` because the binary will derive `Deserialize` for participant JSON.
- Modify `config/src/args.rs`: add `HeadlessConfigArgs`, a `config::Source` implementation, and tests for override collection.
- Modify `config/src/lib.rs`: re-export `HeadlessConfigArgs`, factor config loading into a generic helper, and add `Config::new_headless`.
- Create `src/headless.rs`: own headless CLI args, participant JSON parsing, config layering, logging initialization, participant spawning, and shutdown orchestration.
- Modify `src/main.rs`: add the `headless` subcommand and route it to `headless::run`.

---

### Task 1: Add Headless Config Overrides

**Files:**
- Modify: `config/src/args.rs`
- Modify: `config/src/lib.rs`

- [ ] **Step 1: Write config argument tests**

Add these tests to `config/src/args.rs` under the existing module content:

```rust
#[cfg(test)]
mod tests {
    use super::HeadlessConfigArgs;
    use config::Source as _;

    #[test]
    fn headless_args_collect_only_present_values() {
        let args = HeadlessConfigArgs {
            debug: 2,
            url: Some("https://example.com/space/demo".to_string()),
            backend: Some("cloudflare".to_string()),
            audio_enabled: Some(false),
            video_enabled: None,
            screenshare_enabled: Some(true),
            auto_gain_control: None,
            noise_suppression: Some("rnnoise".to_string()),
            transport: Some("webrtc".to_string()),
            resolution: Some("p720".to_string()),
            blur: Some(true),
            headless: Some(true),
        };

        let values = args.collect().expect("collect headless args");

        assert_eq!(values.get("debug").unwrap().clone().into_int().unwrap(), 2);
        assert_eq!(values.get("url").unwrap().clone().into_string().unwrap(), "https://example.com/space/demo");
        assert_eq!(values.get("backend").unwrap().clone().into_string().unwrap(), "cloudflare");
        assert!(!values.get("audio_enabled").unwrap().clone().into_bool().unwrap());
        assert!(values.get("screenshare_enabled").unwrap().clone().into_bool().unwrap());
        assert_eq!(values.get("noise_suppression").unwrap().clone().into_string().unwrap(), "rnnoise");
        assert_eq!(values.get("transport").unwrap().clone().into_string().unwrap(), "webrtc");
        assert_eq!(values.get("resolution").unwrap().clone().into_string().unwrap(), "p720");
        assert!(values.get("blur").unwrap().clone().into_bool().unwrap());
        assert!(values.get("headless").unwrap().clone().into_bool().unwrap());
        assert!(!values.contains_key("video_enabled"));
        assert!(!values.contains_key("auto_gain_control"));
    }
}
```

- [ ] **Step 2: Run the targeted config test and verify it fails**

Run:

```bash
cargo test -p client-simulator-config headless_args_collect_only_present_values
```

Expected: compile failure because `HeadlessConfigArgs` does not exist.

- [ ] **Step 3: Implement `HeadlessConfigArgs`**

Add this to `config/src/args.rs` after `TuiArgs`:

```rust
/// Non-TUI participant launch options.
#[derive(clap::Args, Default, Debug, Clone)]
pub struct HeadlessConfigArgs {
    /// Verbosity level, set from the parent command.
    #[clap(skip)]
    pub debug: u8,

    /// URL of the hyper.video session.
    #[clap(long, value_name = "URL")]
    pub url: Option<String>,

    /// Participant backend: local, cloudflare, remote-stub, or aws-device-farm.
    #[clap(long, value_name = "BACKEND")]
    pub backend: Option<String>,

    /// Enable or disable browser headless mode.
    #[clap(long = "headless", action)]
    pub headless: Option<bool>,

    /// Enable or disable participant audio.
    #[clap(long = "audio-enabled", action)]
    pub audio_enabled: Option<bool>,

    /// Enable or disable participant video.
    #[clap(long = "video-enabled", action)]
    pub video_enabled: Option<bool>,

    /// Enable or disable participant screenshare.
    #[clap(long = "screenshare-enabled", action)]
    pub screenshare_enabled: Option<bool>,

    /// Enable or disable browser auto gain control.
    #[clap(long = "auto-gain-control", action)]
    pub auto_gain_control: Option<bool>,

    /// Noise suppression model.
    #[clap(long = "noise-suppression", value_name = "MODEL")]
    pub noise_suppression: Option<String>,

    /// Transport mode: webtransport or webrtc.
    #[clap(long, value_name = "MODE")]
    pub transport: Option<String>,

    /// Webcam resolution, such as auto, p720, or p1080.
    #[clap(long, value_name = "RESOLUTION")]
    pub resolution: Option<String>,

    /// Enable or disable background blur.
    #[clap(long, action)]
    pub blur: Option<bool>,
}
```

Then add a `config::Source` implementation parallel to `TuiArgs`, inserting only `Some` values and inserting `debug` only when it is greater than zero.

- [ ] **Step 4: Re-export and add a non-TUI config constructor**

In `config/src/lib.rs`, change:

```rust
pub use args::TuiArgs;
```

to:

```rust
pub use args::{
    HeadlessConfigArgs,
    TuiArgs,
};
```

Refactor `Config::new` so both TUI and headless loading share the same source order:

```rust
impl Config {
    pub fn new(args: TuiArgs) -> Result<Self, config::ConfigError> {
        Self::load_with_overrides(args)
    }

    pub fn new_headless(args: HeadlessConfigArgs) -> Result<Self, config::ConfigError> {
        Self::load_with_overrides(args)
    }

    fn load_with_overrides<S>(overrides: S) -> Result<Self, config::ConfigError>
    where
        S: config::Source + Send + Sync + 'static,
    {
        let data_dir = get_data_dir();
        let config_dir = get_config_dir();
        let mut builder = config::Config::builder()
            .set_default("data_dir", data_dir.to_str().unwrap())?
            .set_default("config_dir", config_dir.to_str().unwrap())?;

        builder = builder.add_source(Config::default());

        let config_files = [("config.yaml", config::FileFormat::Yaml)];

        for (file, format) in &config_files {
            let source = config::File::from(config_dir.join(file))
                .format(*format)
                .required(false);
            builder = builder.add_source(source);
        }

        builder = builder.add_source(overrides);

        builder.build()?.try_deserialize()
    }
}
```

- [ ] **Step 5: Run config tests**

Run:

```bash
cargo test -p client-simulator-config
```

Expected: all config crate tests pass.

- [ ] **Step 6: Commit**

```bash
git add config/src/args.rs config/src/lib.rs
git commit -m "feat: add headless config overrides"
```

---

### Task 2: Add Participant JSON Overlay Logic

**Files:**
- Modify: `Cargo.toml`
- Create: `src/headless.rs`

- [ ] **Step 1: Add serde to the root crate**

In the root `Cargo.toml`, add:

```toml
serde.workspace = true
```

- [ ] **Step 2: Write participant overlay tests**

Create `src/headless.rs` with the test module first:

```rust
use client_simulator_config::{
    CloudflareConfig,
    Config,
    DeviceFarmConfig,
    HeadlessConfigArgs,
    NoiseSuppression,
    ParticipantBackendKind,
    TransportMode,
    WebcamResolution,
};
use eyre::{
    Context as _,
    Result,
};
use serde::Deserialize;

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn participant_json_overrides_global_config() {
        let global = Config {
            url: Some(Url::parse("https://example.com/global").unwrap()),
            backend: ParticipantBackendKind::Local,
            audio_enabled: true,
            video_enabled: true,
            transport: TransportMode::WebTransport,
            ..Default::default()
        };

        let participant = parse_participant_override(
            r#"{
                "url": "https://example.com/participant",
                "backend": "cloudflare",
                "audio_enabled": false,
                "transport": "webrtc",
                "resolution": "p720",
                "blur": true
            }"#,
        )
        .expect("valid participant json");

        let effective = participant.apply_to(global);

        assert_eq!(effective.url.unwrap().as_str(), "https://example.com/participant");
        assert_eq!(effective.backend, ParticipantBackendKind::Cloudflare);
        assert!(!effective.audio_enabled);
        assert!(effective.video_enabled);
        assert_eq!(effective.transport, TransportMode::WebRTC);
        assert_eq!(effective.resolution, WebcamResolution::P720);
        assert!(effective.blur);
    }

    #[test]
    fn empty_participant_list_starts_one_default_participant() {
        let global = Config {
            url: Some(Url::parse("https://example.com/global").unwrap()),
            audio_enabled: true,
            ..Default::default()
        };

        let configs = participant_configs(global, &[]).expect("participant configs");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].url.as_ref().unwrap().as_str(), "https://example.com/global");
        assert!(configs[0].audio_enabled);
    }

    #[test]
    fn unknown_participant_json_field_is_rejected() {
        let err = parse_participant_override(r#"{"audio_enable": false}"#).unwrap_err();

        assert!(format!("{err:?}").contains("audio_enable"));
    }
}
```

- [ ] **Step 3: Run the overlay tests and verify they fail**

Run:

```bash
cargo test participant_json_overrides_global_config empty_participant_list_starts_one_default_participant unknown_participant_json_field_is_rejected
```

Expected: compile failure because the tested functions and types do not exist.

- [ ] **Step 4: Implement participant overlay types**

Add this implementation above the tests in `src/headless.rs`:

```rust
#[derive(clap::Args, Debug, Clone, Default)]
pub struct HeadlessArgs {
    #[command(flatten)]
    pub config: HeadlessConfigArgs,

    /// Participant-specific JSON overrides. Repeat this flag to start multiple participants.
    #[clap(long = "participant", value_name = "JSON")]
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ParticipantOverride {
    url: Option<url::Url>,
    backend: Option<ParticipantBackendKind>,
    cloudflare: Option<CloudflareConfig>,
    device_farm: Option<DeviceFarmConfig>,
    headless: Option<bool>,
    audio_enabled: Option<bool>,
    video_enabled: Option<bool>,
    screenshare_enabled: Option<bool>,
    auto_gain_control: Option<bool>,
    noise_suppression: Option<NoiseSuppression>,
    transport: Option<TransportMode>,
    resolution: Option<WebcamResolution>,
    blur: Option<bool>,
}

impl ParticipantOverride {
    fn apply_to(&self, mut config: Config) -> Config {
        if let Some(url) = &self.url {
            config.url = Some(url.clone());
        }
        if let Some(backend) = self.backend {
            config.backend = backend;
        }
        if let Some(cloudflare) = &self.cloudflare {
            config.cloudflare = cloudflare.clone();
        }
        if let Some(device_farm) = &self.device_farm {
            config.device_farm = device_farm.clone();
        }
        if let Some(headless) = self.headless {
            config.headless = headless;
        }
        if let Some(audio_enabled) = self.audio_enabled {
            config.audio_enabled = audio_enabled;
        }
        if let Some(video_enabled) = self.video_enabled {
            config.video_enabled = video_enabled;
        }
        if let Some(screenshare_enabled) = self.screenshare_enabled {
            config.screenshare_enabled = screenshare_enabled;
        }
        if let Some(auto_gain_control) = self.auto_gain_control {
            config.auto_gain_control = auto_gain_control;
        }
        if let Some(noise_suppression) = self.noise_suppression {
            config.noise_suppression = noise_suppression;
        }
        if let Some(transport) = self.transport {
            config.transport = transport;
        }
        if let Some(resolution) = self.resolution {
            config.resolution = resolution;
        }
        if let Some(blur) = self.blur {
            config.blur = blur;
        }
        config
    }
}

fn parse_participant_override(json: &str) -> Result<ParticipantOverride> {
    serde_json::from_str(json).wrap_err_with(|| format!("Failed to parse --participant JSON: {json}"))
}

fn participant_configs(global_config: Config, participant_json: &[String]) -> Result<Vec<Config>> {
    if participant_json.is_empty() {
        return Ok(vec![global_config]);
    }

    participant_json
        .iter()
        .map(|json| parse_participant_override(json).map(|participant| participant.apply_to(global_config.clone())))
        .collect()
}
```

- [ ] **Step 5: Run the overlay tests**

Run:

```bash
cargo test participant_json_overrides_global_config empty_participant_list_starts_one_default_participant unknown_participant_json_field_is_rejected
```

Expected: all three tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/headless.rs
git commit -m "feat: parse headless participant overrides"
```

---

### Task 3: Implement Headless Runtime and Shutdown Semantics

**Files:**
- Modify: `src/headless.rs`

- [ ] **Step 1: Write shutdown behavior tests**

Add these tests to the existing test module in `src/headless.rs`:

```rust
use client_simulator_browser::participant::ParticipantStore;
use std::{
    fs,
    path::PathBuf,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};
use tokio::sync::oneshot;

#[tokio::test]
async fn shutdown_wait_returns_clean_when_participants_stop() {
    let store = ParticipantStore::new(unique_test_data_dir());
    let config = Config {
        url: Some(Url::parse("https://example.com/lite/demo").unwrap()),
        backend: ParticipantBackendKind::RemoteStub,
        ..Default::default()
    };
    store.spawn(&config).expect("spawn participant");

    let (first_tx, first_rx) = oneshot::channel::<()>();
    let (_second_tx, second_rx) = oneshot::channel::<()>();
    let store_for_shutdown = store.clone();

    tokio::spawn(async move {
        first_tx.send(()).expect("send first shutdown signal");
    });

    let result = wait_for_headless_exit(
        store_for_shutdown,
        async move {
            let _ = first_rx.await;
        },
        || async move {
            let _ = second_rx.await;
        },
    )
    .await;

    assert_eq!(result, HeadlessExit::Clean);
    assert!(store.is_empty());
}

#[tokio::test]
async fn second_shutdown_signal_returns_forced() {
    let store = ParticipantStore::new(unique_test_data_dir());
    let config = Config {
        url: Some(Url::parse("https://example.com/lite/demo").unwrap()),
        backend: ParticipantBackendKind::RemoteStub,
        ..Default::default()
    };
    store.spawn(&config).expect("spawn participant");

    let (first_tx, first_rx) = oneshot::channel::<()>();
    let (second_tx, second_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        first_tx.send(()).expect("send first shutdown signal");
        second_tx.send(()).expect("send second shutdown signal");
    });

    let result = wait_for_headless_exit(
        store,
        async move {
            let _ = first_rx.await;
        },
        || async move {
            let _ = second_rx.await;
        },
    )
    .await;

    assert_eq!(result, HeadlessExit::Forced);
}

fn unique_test_data_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("hyper-browser-simulator-headless-test-{timestamp}"));
    fs::create_dir_all(&path).expect("create test data dir");
    path
}
```

- [ ] **Step 2: Run the shutdown tests and verify they fail**

Run:

```bash
cargo test shutdown_wait_returns_clean_when_participants_stop second_shutdown_signal_returns_forced
```

Expected: compile failure because `HeadlessExit` and `wait_for_headless_exit` do not exist.

- [ ] **Step 3: Implement runtime entry points**

Add this to `src/headless.rs`:

```rust
use client_simulator_browser::participant::ParticipantStore;
use std::{
    future::Future,
};
use tracing_subscriber::{
    fmt::{
        self,
        writer::MakeWriterExt as _,
    },
    prelude::*,
    registry,
    EnvFilter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadlessExit {
    Clean,
    Forced,
}

pub async fn run(mut args: HeadlessArgs, debug: u8) -> Result<i32> {
    args.config.debug = debug;
    init_headless_logging(debug)?;

    let global_config = Config::new_headless(args.config).context("Failed to create headless config")?;
    let participant_configs = participant_configs(global_config.clone(), &args.participants)?;
    let store = ParticipantStore::new(global_config.data_dir());

    for config in &participant_configs {
        store
            .spawn(config)
            .wrap_err_with(|| format!("Failed to spawn participant with backend {}", config.backend))?;
    }

    let exit = wait_for_headless_exit(store, wait_for_ctrl_c(), wait_for_ctrl_c).await;

    Ok(match exit {
        HeadlessExit::Clean => 0,
        HeadlessExit::Forced => 130,
    })
}

fn init_headless_logging(debug: u8) -> Result<()> {
    let filter = match debug {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let env_filter = EnvFilter::builder().parse_lossy(filter);
    let writer = std::io::stderr.with_min_level(tracing::Level::WARN).or_else(std::io::stdout);

    registry()
        .with(tracing_error::ErrorLayer::default())
        .with(fmt::layer().with_writer(writer).with_filter(env_filter))
        .try_init()
        .context("Failed to initialize headless logging")
}

async fn wait_for_ctrl_c() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!("Failed to listen for Ctrl-C: {err}");
    }
}

async fn wait_for_headless_exit<F1, F2, SecondShutdown>(
    store: ParticipantStore,
    first_shutdown: F1,
    second_shutdown: SecondShutdown,
) -> HeadlessExit
where
    F1: Future<Output = ()>,
    SecondShutdown: FnOnce() -> F2,
    F2: Future<Output = ()>,
{
    tokio::pin!(first_shutdown);

    let mut state_receivers = store
        .values()
        .into_iter()
        .map(|participant| participant.state.clone())
        .collect::<Vec<_>>();

    let all_stopped = async move {
        for state in &mut state_receivers {
            let _ = state.wait_for(|current| !current.running).await;
        }
    };
    tokio::pin!(all_stopped);

    tokio::select! {
        _ = &mut all_stopped => HeadlessExit::Clean,
        _ = &mut first_shutdown => {
            let shutdown = store.shutdown_all();
            let second_shutdown = second_shutdown();
            tokio::pin!(shutdown);
            tokio::pin!(second_shutdown);
            tokio::select! {
                _ = &mut shutdown => HeadlessExit::Clean,
                _ = &mut second_shutdown => HeadlessExit::Forced,
            }
        }
    }
}
```

- [ ] **Step 4: Run the shutdown tests**

Run:

```bash
cargo test shutdown_wait_returns_clean_when_participants_stop second_shutdown_signal_returns_forced
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/headless.rs
git commit -m "feat: add headless runner shutdown handling"
```

---

### Task 4: Wire the CLI Command

**Files:**
- Modify: `src/main.rs`
- Modify: `src/headless.rs`

- [ ] **Step 1: Write CLI parsing tests**

Add this test module to `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{
        CliArgs,
        Command,
    };
    use clap::Parser as _;

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
                assert_eq!(headless.config.url.as_deref(), Some("https://latest.dev.hyper.video/F27-T5F-DXY"));
                assert_eq!(headless.participants.len(), 2);
            }
            other => panic!("expected headless command, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run the CLI parsing test and verify it fails**

Run:

```bash
cargo test parses_headless_command_with_repeated_participants
```

Expected: compile failure because `headless` is not a module and `Command::Headless` does not exist.

- [ ] **Step 3: Wire `headless` into `main.rs`**

At the top of `src/main.rs`, add:

```rust
mod headless;
```

Extend `Command`:

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

Derive `Debug` for `CliArgs` if the test needs it:

```rust
#[derive(Parser, Debug)]
```

Route the command in `main`:

```rust
        Some(Command::Headless(args)) => {
            let code = headless::run(args, debug).await?;
            std::process::exit(code);
        }
```

- [ ] **Step 4: Run the CLI parsing test**

Run:

```bash
cargo test parses_headless_command_with_repeated_participants
```

Expected: the test passes.

- [ ] **Step 5: Run a smoke test with the remote stub backend**

Run:

```bash
cargo run -- headless \
  --url "https://example.com/lite/demo" \
  --participant '{"backend": "remote-stub", "audio_enabled": false}'
```

Expected: the process starts, logs appear in the terminal, pressing `Ctrl-C` once shuts down the participant and exits. Run it a second time, press `Ctrl-C` twice quickly, and verify the process exits with code `130`.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/headless.rs
git commit -m "feat: add headless cli command"
```

---

### Task 5: Full Verification

**Files:**
- No new files.

- [ ] **Step 1: Format**

Run:

```bash
just fmt
```

Expected: formatting completes without errors.

- [ ] **Step 2: Lint**

Run:

```bash
just clippy
```

Expected: clippy completes with no warnings.

- [ ] **Step 3: Test**

Run:

```bash
just test
```

Expected: all tests pass.

- [ ] **Step 4: Manual command check**

Run:

```bash
cargo run -- headless \
  --url "https://latest.dev.hyper.video/F27-T5F-DXY" \
  --participant '{"audio_enabled": false, "backend": "local"}' \
  --participant '{"audio_enabled": true, "backend": "cloudflare"}'
```

Expected: two participants are created, logs stream to the terminal, the app does not enter TUI mode, no config file is written as a result of the command-line overrides, one `Ctrl-C` starts clean shutdown, and a second `Ctrl-C` exits immediately with code `130`.

- [ ] **Step 5: Commit verification fixes if any were needed**

If formatting, linting, or tests required changes:

```bash
git add Cargo.toml config/src/args.rs config/src/lib.rs src/headless.rs src/main.rs
git commit -m "fix: verify headless command"
```

Skip this commit if no files changed during verification.

---

## Self-Review

- Spec coverage: The plan covers the new `headless` command, no config persistence, config/default loading, global CLI overrides, per-participant JSON overrides with highest priority, stdout/stderr tracing, graceful shutdown on first `Ctrl-C`, and forced exit on second `Ctrl-C`.
- Placeholder scan: No incomplete implementation steps remain; tests, commands, expected results, file paths, and concrete code shapes are provided.
- Type consistency: `HeadlessConfigArgs`, `HeadlessArgs`, `ParticipantOverride`, `HeadlessExit`, and `wait_for_headless_exit` are introduced before they are used by later tasks.
