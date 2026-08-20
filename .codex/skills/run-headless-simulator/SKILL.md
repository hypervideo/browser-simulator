---
name: run-headless-simulator
description: Run hyper-client-simulator participants from the CLI without the TUI. Use for agent-friendly headless smoke tests, multi-participant runs, and backend startup checks.
---

# Run Headless Simulator

## Overview

Use the `headless` subcommand when an agent or script needs to run simulator participants without the interactive TUI. In the repo, prefer the development command through Cargo. For an installed release, call the binary directly.

```sh
cargo run -- headless [OPTIONS]
hyper-client-simulator headless [OPTIONS]
```

Run from the Nix devshell when possible so Chromium, FFmpeg, native libraries, and the Rust toolchain match the repository environment.

## Basic Usage

Start one participant from global CLI settings:

```sh
cargo run -- headless \
  --url https://latest.dev.hyper.video/0Y6-6FZ-20J \
  --backend local \
  --headless true \
  --audio-enabled true \
  --video-enabled true
```

Start one backend-only smoke test that does not require Chromium:

```sh
cargo run -- headless \
  --url https://example.com/lite/demo \
  --participant '{"backend":"remote-stub","audio_enabled":false}'
```

Start multiple participants by repeating `--participant`:

```sh
cargo run -- headless \
  --url https://latest.dev.hyper.video/F27-T5F-DXY \
  --participant '{"backend":"local","audio_enabled":false}' \
  --participant '{"backend":"cloudflare","audio_enabled":true}'
```

## Configuration Model

- Global CLI flags apply to the base config loaded by the simulator.
- If the command receives no `--participant` values, `headless` starts one participant from the effective global config.
- Each repeated `--participant` JSON value clones the effective global config and applies only the overrides for that participant.
- CLI overrides and participant JSON overrides are in-memory only. `headless` does not persist them back to the config file.
- Logs print through normal tracing output rather than the TUI logger.
- First `Ctrl-C` requests shutdown and waits for participants to stop. A second `Ctrl-C` exits with code `130`.

## Options

Global CLI option names use kebab-case:

- `--url URL`
- `--backend BACKEND`
- `--headless true|false`
- `--browser-logs true|false` (default: `true`)
- `--audio-enabled true|false`
- `--video-enabled true|false`
- `--screenshare-enabled true|false`
- `--auto-gain-control true|false`
- `--noise-suppression MODEL`
- `--transport MODE`
- `--video-constraint-publish-webcam CONSTRAINT`
- `--video-constraint-subscribe CONSTRAINT`
- `--video-max-concurrent-tracks TRACKS`
- `--blur true|false`
- `--participant JSON`

Participant JSON field names use snake_case:

- `url`
- `backend`
- `headless`
- `browser_logs`
- `audio_enabled`
- `video_enabled`
- `screenshare_enabled`
- `auto_gain_control`
- `noise_suppression`
- `transport`
- `video_constraint_publish_webcam`
- `video_constraint_subscribe`
- `video_max_concurrent_tracks`
- `blur`

Participant JSON denies unknown fields. Treat typos as command errors, not ignored settings.

## Values

Backend values:

- `local`
- `cloudflare`
- `remote-stub`
- `aws-device-farm`

Transport values:

- `webtransport`
- `webrtc`

Video constraint values:

- `none`
- `90p`
- `144p`
- `240p`
- `360p`
- `480p`
- `720p`
- `1080p`
- `1440p`
- `2160p`

Noise suppression values:

- `none`
- `deepfilternet`
- `rnnoise`
- `iris-carthy`
- `krisp-high`
- `krisp-medium`
- `krisp-low`
- `krisp-high-with-bvc`
- `krisp-medium-with-bvc`
- `ai-coustics-sparrow-xxs`
- `ai-coustics-sparrow-xs`
- `ai-coustics-sparrow-s`
- `ai-coustics-sparrow-l`
- `ai-coustics-sparrow-xxs-48khz`
- `ai-coustics-sparrow-xs-48khz`
- `ai-coustics-rook-s-48khz`
- `ai-coustics-rook-l-48khz`

Use an integer for `--video-max-concurrent-tracks` or JSON `video_max_concurrent_tracks`. In participant JSON, `null` means absent, so the participant inherits the global value. It does not clear a global track limit.

## Browser Logs

Headless runs collect browser logs by default. Set `--browser-logs false` to
disable collection for every participant. Set `"browser_logs": false` in the
JSON for one participant to disable collection only for that participant.

Each browser backend captures the logs available through its browser control
protocol:

- `local` captures `console.*` calls, uncaught exceptions, unhandled
  rejections, and browser entries such as failed requests, CORS errors,
  security warnings, and deprecations.
- `aws-device-farm` captures ChromeDriver browser logs, including console
  calls, JavaScript exceptions, and network or security entries.
- `cloudflare` captures page console calls and uncaught page errors through
  Playwright in the worker.
- `remote-stub` does not run a browser and has no browser logs.

Every browser message includes the participant span before the `browser`
tracing target and text. The `source` field is `console`, `exception`, or
`browser`:

```text
INFO participant{name=local-fox-3}: browser: transport connected source=console
```

The default `info` filter hides `console.debug` and similar debug messages. Pass
`--logging debug` before the `headless` subcommand to show them. Use
`RUST_LOG=info,browser=warn` to keep browser warnings and errors only. Use
`RUST_LOG=info,browser=off` to hide browser lines without stopping collection,
or use `--browser-logs false` to stop collection.

## Agent Workflow

1. When a browser participant fails to join or loses media, read its `browser:` lines first.
2. Prefer `remote-stub` for a fast smoke test when browser automation is not the thing under test.
3. Use `local` for real Chromium-driven joining, and confirm the target Hyper frontend exposes the media settings used by video constraints.
4. Put common settings in global CLI flags, then use repeated `--participant` JSON for per-participant differences.
5. Keep JSON compact and quote it with single quotes in shell commands.
6. Check `cargo run -- headless --help` or `hyper-client-simulator headless --help` if the command rejects a flag.
