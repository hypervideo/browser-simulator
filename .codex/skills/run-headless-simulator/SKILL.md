---
name: run-headless-simulator
description: Run hyper-client-simulator participants from the CLI without the TUI. Use for headless smoke tests, multi-participant runs, browser-log debugging, backend checks, and remote-session cleanup.
---

# Run Headless Simulator

Use the `headless` subcommand to run participants without the TUI. The subcommand does not hide the browser window. Use `--headless true` to hide a local Chrome or Chromium window. Cloudflare is always headless. AWS Device Farm and `remote-stub` do not use this setting.

## Run the Simulator

Run development commands through the Nix development shell:

```sh
nix develop --command just dev --logging info headless [OPTIONS]
```

If the shell is already open, use the shorter command:

```sh
just dev --logging info headless [OPTIONS]
```

For an installed release, run:

```sh
hyper-client-simulator --logging info headless [OPTIONS]
```

Run the Nix package from a checkout with:

```sh
just run-nix --logging info headless [OPTIONS]
```

Run it without a checkout with:

```sh
nix run 'git+https://github.com/hypervideo/browser-simulator.git?submodules=1' -- --logging info headless [OPTIONS]
```

Keep `submodules=1`. The build needs the Cloudflare worker submodule.

The examples below assume that the Nix development shell is already open.

### Start One Local Participant

```sh
just dev --logging info headless \
  --url https://latest.dev.hyper.video/m/SESSION_ID \
  --backend local \
  --headless true \
  --audio-enabled true \
  --video-enabled true
```

### Test the CLI Without a Browser

`remote-stub` does not open a browser or contact a server. It still needs a URL.

```sh
just dev --logging info headless \
  --url https://example.com/m/demo \
  --backend remote-stub \
  --audio-enabled false
```

### Start Multiple Participants

Repeat `--participant` for each participant. Quote each JSON object with single quotes.

```sh
just dev --logging info headless \
  --url https://latest.dev.hyper.video/m/SESSION_ID \
  --headless true \
  --participant '{"backend":"local","audio_enabled":false}' \
  --participant '{"backend":"cloudflare","transport":"webrtc","audio_enabled":true}'
```

## Understand the Settings

The command applies settings in this order:

1. Start with the built-in defaults.
2. Load `config.yaml`.
3. Apply the headless options.
4. Apply the JSON overrides for each participant.

A later value wins. Browser logs are the exception. The `headless` subcommand sets them to `true` unless you pass `--browser-logs false`.

Each participant needs a session URL from `config.yaml`, `--url`, or its JSON object. A `/m` or `/m/...` path selects Hyper Lite. Other paths select Hyper Core and may require a session cookie.

With no `--participant`, the command starts one participant from the shared settings. With one or more `--participant` values, it starts only those participants. It does not start an extra participant from the shared settings.

An omitted or `null` JSON field inherits the shared value. It cannot clear an optional shared value. Unknown fields make the command fail. CLI and JSON overrides are not saved.

Fake-media selection and nested Cloudflare or Device Farm settings have no headless options. Set them in `config.yaml`. They apply to every participant in the run.

## Choose a Backend

- `local` runs a real Chrome or Chromium browser on this machine. It is the only backend that uses `--headless`.
- `cloudflare` uses the worker at `cloudflare.base_url`. The default is the staging worker. Worker browsers are always headless and use WebRTC. This backend ignores local fake-media files and uses worker-provided media.
- `aws-device-farm` creates a billable remote Test Grid session. Startup can take 60 to 120 seconds. This backend uses synthetic media and does not use `--headless`.
- `remote-stub` runs no browser and sends no network requests. Use it only to test configuration and process lifecycle.

Device Farm needs the `hyper-client-simulator` AWS profile or both of these environment variables:

- `DEVICE_FARM_AWS_ACCESS_KEY_ID`
- `DEVICE_FARM_AWS_SECRET_ACCESS_KEY`

Use `just dev aws setup-auth` to load the profile from 1Password. This command writes the profile to `~/.aws/credentials`. The project and region come from `device_farm` in `config.yaml` or from `DEVICE_FARM_PROJECT_ARN` and `DEVICE_FARM_AWS_REGION`.

Before a large Cloudflare run, check capacity with `just dev --logging info cf limits`. The `just dev --logging info cf sessions` command shows open sessions and browser preview links. To use another worker for a headless run, set `cloudflare.base_url` in `config.yaml`. A `--base-url` option on a `cf` management command affects only that call.

## Read Browser Logs

The `headless` subcommand collects browser logs by default. `--browser-logs false` changes the shared value. A participant can override it with `"browser_logs": true` or `false`.

Each backend collects the logs available through its browser control protocol:

- `local` collects `console.*` calls, uncaught exceptions, unhandled rejections, failed requests, CORS errors, security warnings, and deprecations.
- `aws-device-farm` collects ChromeDriver browser logs. These include console calls, JavaScript exceptions, and network or security entries.
- `cloudflare` collects page console calls and uncaught page errors through Playwright.
- `remote-stub` has no browser logs.

Each line includes the generated participant name, the `browser` target, the message, and a `source` value of `console`, `exception`, or `browser`:

```text
INFO participant{name=local-fox-3}: browser: transport connected source=console
```

Generated names start with `local-`, `cf-`, `aws-`, or `stub-`. Use the prefix to match a line to its backend.

The simulator limits each message to 4 KiB. A polled Device Farm or Cloudflare batch keeps the newest 500 entries. The simulator writes a warning when it drops older entries. Remote log entries can arrive on the next poll or during shutdown.

Browser logs use the normal tracing filter. Put the global `--logging` option before `headless`. It accepts `error`, `warn`, `info`, `debug`, or `trace` and overrides `RUST_LOG`.

- `--logging debug` shows debug browser messages.
- `nix develop --command env RUST_LOG=info,browser=warn just dev headless ...` shows only browser warnings and errors.
- `nix develop --command env RUST_LOG=info,browser=off just dev headless ...` hides browser lines but still collects them.
- `--browser-logs false` stops collection unless a participant JSON override enables it again.

When both logging controls are absent, the binary uses `info`. The Nix shell for this repository defines `RUST_LOG=debug`. The examples use `--logging info` to give stable output.

Trace, debug, and info lines go to stdout. Warning and error lines go to stderr. Append `> run.log 2>&1` to a command to save both streams.

## Stop and Clean Up

There is no duration option. The command runs until every participant stops or it receives `Ctrl-C`. `remote-stub` does not stop on its own.

1. Wait for each real participant to start. Local and Device Farm log `Joined the space`. Cloudflare logs `Created Cloudflare worker session` with its session ID.
2. Check `ERROR` lines as well as the exit status. Some runtime startup failures can stop a participant and still let the command exit with status `0`.
3. Press `Ctrl-C` once.
4. Wait for every participant and remote session to close.
5. Use a second `Ctrl-C` only when cleanup cannot finish. It exits with status `130` and may leave remote sessions open.

After a forced exit, inspect only the backend used by the run:

```sh
just dev --logging info cf sessions
just dev --logging info cf close SESSION_ID

just dev --logging info aws list-sessions --status active --since '1 hour'
just dev --logging info aws close-sessions SESSION_ID
```

Close only session IDs created by the run. Do not use `cf close all` or `aws close-sessions` without an ID unless the user asks for that scope and you verify it first.

## Headless Option Reference

Headless option names use kebab-case. Participant JSON fields use snake_case.

| Headless option | Participant JSON field |
| --- | --- |
| `--url URL` | `"url"` |
| `--backend BACKEND` | `"backend"` |
| `--headless true\|false` | `"headless"` |
| `--browser-logs true\|false` | `"browser_logs"` |
| `--audio-enabled true\|false` | `"audio_enabled"` |
| `--video-enabled true\|false` | `"video_enabled"` |
| `--screenshare-enabled true\|false` | `"screenshare_enabled"` |
| `--auto-gain-control true\|false` | `"auto_gain_control"` |
| `--noise-suppression MODEL` | `"noise_suppression"` |
| `--transport MODE` | `"transport"` |
| `--video-constraint-publish-webcam CONSTRAINT` | `"video_constraint_publish_webcam"` |
| `--video-constraint-subscribe CONSTRAINT` | `"video_constraint_subscribe"` |
| `--video-max-concurrent-tracks TRACKS` | `"video_max_concurrent_tracks"` |
| `--blur true\|false` | `"blur"` |

Repeat `--participant JSON` to add participants. It has no matching JSON field.

Use a nonnegative integer for `--video-max-concurrent-tracks` or `video_max_concurrent_tracks`. JSON `null` inherits the shared limit. It does not remove that limit.

### Accepted Values

Backend:

- `local`
- `cloudflare`
- `remote-stub`
- `aws-device-farm`

Transport:

- `webtransport`
- `webrtc`

Video constraint:

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

Noise suppression:

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

Run `just dev --logging info headless --help` or `hyper-client-simulator headless --help` if a flag fails.
