# Plan: Cloudflare Driver For `hyper-browser-simulator`

## Recommendation

The generic backend seam already exists in the current codebase:

- `ParticipantDriverSession` in `browser/src/participant/shared/runtime.rs` is the real backend contract.
- `FrontendAutomation` in `browser/src/participant/local/frontend.rs` is now local-Chromium-only.

So the Cloudflare work should build on the current `ParticipantDriverSession` runtime instead of reviving the older concrete `FrontendDriver` idea.

Recommended shape:

- Keep the actual driver implementation in `client-simulator-browser`, under `browser/src/participant/cloudflare/`.
- Add one new workspace crate for the generated worker client, for example `cloudflare-worker-client/`.
- Do not put the driver implementation itself in a separate crate. That would create a dependency cycle because `client-simulator-browser` owns the `ParticipantDriverSession` trait and also needs to construct the driver.
- Reuse the existing Rust auth flow for Hyper Core. The driver should obtain or reuse the `hyper_session` cookie locally and pass the raw cookie value to the worker during session creation.
- Treat the worker's `sessionId` as a private implementation detail of the driver. Nothing above `CloudflareSession` should know about it.
- Cache authoritative state from worker responses and use a low-frequency worker state poll to implement `wait_for_termination()`. Note that the cloudflare Browser Rendering instance will terminate after its keep-alive timeout passes without inactivity. The driver and cloudflare worker needs to connect regularly to the instance to keep it alive.

## Related Project

The counterpart worker project lives at:

- `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator`

The most relevant files in that repo for this plan are:

- worker/API entrypoint: `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/worker/src/index.ts`
- worker routes and schemas: `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/worker/src/api/`
- current worker automation logic: `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/worker/src/api/logic.ts`
- current generated-client example: `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/cli/`
- counterpart implementation plan: `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/plans/2026-04-16_hyper-browser-simulator-support.md`

## Current State

What is already in place:

- The shared runtime and driver contract are implemented.
- `LocalChromiumSession` is the production local backend.
- `RemoteStubSession` is a simulated remote backend.
- `browser/src/participant/cloudflare/mod.rs` is an empty placeholder.
- `ParticipantBackendKind` only supports `local` and `remote-stub`.
- The TUI already has backend selection.

What is missing for a real Cloudflare backend:

- a generated Rust client for the worker API in this repo
- a `CloudflareSession` implementation of `ParticipantDriverSession`
- config for the worker base URL and request behavior
- spawn/store wiring for the new backend
- tests against a mock worker

The cloudflare-browser-simulator repo already has a worker implementation following `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/plans/2026-04-16_hyper-browser-simulator-support.md`. You can run a local worker using the justfile commands at `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/justfile`. If the worker API is not sufficient or could be made more ergonomic for the driver, you can modify its implementation and OpenAPI spec as needed.

## Design Decisions

### Use a generated client crate, not hand-written HTTP

The worker repo already treats its OpenAPI document as canonical. Mirror that pattern here.

Recommended crate layout:

- `cloudflare-worker-client/`
  - `build.rs`
  - `openapi/cloudflare-browser-simulator.json`
  - `src/generated.rs`
  - `src/lib.rs`
  - `src/client.rs`

Responsibilities:

- own the committed OpenAPI copy used by this repo
- generate the Rust client with `progenitor`
- expose a thin ergonomic wrapper around the generated client
- keep `reqwest` transport, timeout, base URL, and error formatting out of `browser/`

### Keep the Cloudflare driver in `browser/`

Recommended files:

- `browser/src/participant/cloudflare/mod.rs`
- `browser/src/participant/cloudflare/session.rs`
- `browser/src/participant/cloudflare/config.rs`
- `browser/src/participant/cloudflare/mapping.rs`

Responsibilities:

- map `ParticipantLaunchSpec` to worker create-session requests
- map `ParticipantMessage` to worker command requests
- translate worker state into `ParticipantState`
- forward worker log entries into `ParticipantLogMessage`
- own `sessionId`, cached state, and termination polling

### Reuse the existing auth stack

For Hyper Core:

- use `HyperSessionCookieManger` exactly as the local backend does
- if a cookie is already available, reuse it
- otherwise fetch one with `fetch_new_cookie(base_url, username)`
- send only the cookie value to the worker

The worker should set that cookie into the browser context before navigation. Do not duplicate the guest-auth flow inside the worker.

### Keep backend-specific limitations at the driver boundary

Cloudflare differs from local Chromium in a few important ways:

- always headless
- no local user data dir
- no local fake-media files from the Rust host
- WebRTC-only in practice

The driver should absorb those differences by:

- logging when a setting is ignored or normalized
- exposing the actual applied state from the worker
- not changing the `Participant`, TUI command surface, or shared runtime just to model Cloudflare internals

## Scope

In scope for this implementation:

- new `cloudflare` backend kind
- Hyper Core and Hyper Lite support through the worker
- full `ParticipantMessage` coverage
- accurate `ParticipantState` refreshes
- proper close and unexpected-termination handling
- generated OpenAPI client in this repo

Explicit non-goals for v1:

- headed Cloudflare sessions
- remote support for local fake-media files or URLs
- sharing DOM selector code across Rust and TypeScript through a new abstraction layer
- exposing worker-specific identifiers outside the driver

## Progress Tracker

Overall status: `phase 1 complete`

Cross-repo dependency:

- This plan depends on the worker contract described in `/Users/robert/projects/shuttle/simulator/cloudflare-browser-simulator/plans/2026-04-16_hyper-browser-simulator-support.md`.

Milestones:

- [x] Phase 1: Freeze the worker contract and add the generated-client crate
- [ ] Phase 2: Add Cloudflare backend config and spawn wiring
- [ ] Phase 3: Implement `CloudflareSession` start and close
- [ ] Phase 4: Implement command handling, cached state, and termination polling
- [ ] Phase 5: Add TUI and UX handling for backend-specific limitations
- [ ] Phase 6: Validate with unit, integration, and manual tests

## Detailed Plan

### Phase 1: Freeze the worker contract and add the generated-client crate

Goal:

- make the worker API a typed dependency of this repo
- avoid hand-written request and response structs

Changes:

- add a new workspace member, for example `cloudflare-worker-client`
- add `progenitor`, `progenitor-client`, `openapiv3`, `prettyplease`, `syn`, and the runtime `reqwest` pieces needed for generated code
- copy the canonical worker spec into this repo under the new crate
- add `build.rs` to generate the client from the committed spec
- expose a small wrapper API for:
  - constructing a client from base URL and timeouts
  - formatting API errors into `eyre::Report`
  - convenient methods for create, command, state, and close calls

TDD steps:

- [x] add a failing test for client construction and base URL normalization
- [x] add a failing test for worker error translation into actionable Rust errors
- [x] implement the wrapper until those tests pass

Implemented in this phase:

- added the `cloudflare-worker-client` workspace crate
- copied the canonical worker OpenAPI spec into `cloudflare-worker-client/openapi/cloudflare-browser-simulator.json`
- added `build.rs`-driven client generation with `progenitor`
- exposed a thin wrapper with base URL normalization, typed session helpers, and `eyre` error formatting
- added focused unit tests covering base URL normalization and actionable worker error translation

Completion criteria:

- `client-simulator-browser` can depend on the generated client crate without any direct OpenAPI or `reqwest` boilerplate
- the worker contract is represented by generated Rust types in this repo

### Phase 2: Add Cloudflare backend config and spawn wiring

Goal:

- make Cloudflare a first-class backend selection

Recommended config shape:

```yaml
backend: cloudflare
cloudflare:
  base_url: https://cloudflare-browser-simulator.hyper-video.workers.dev
  request_timeout_seconds: 30
  session_timeout_ms: 600000
  navigation_timeout_ms: 45000
  selector_timeout_ms: 20000
  debug: false
  health_poll_interval_ms: 5000
```

Recommended code changes:

- extend `ParticipantBackendKind` in `config/src/client_config.rs` with `Cloudflare`
- add a `CloudflareConfig` struct in `config/`
- update `config/src/lib.rs` and `config/src/default-config.yaml`
- update the TUI backend picker to include `cloudflare`
- replace separate `spawn_local()` and `spawn_remote_stub()` calls from the TUI with one backend-dispatching store method to stay DRY
- add `ParticipantStore::spawn(config)` or equivalent central dispatch
- add `Participant::spawn_cloudflare(...)` or equivalent internal constructor

Recommended simplification:

- do not add a large new TUI editor for every Cloudflare field in the first patch
- keep backend selection in the TUI
- keep advanced Cloudflare settings configurable through YAML first

TDD steps:

- [ ] add failing config parsing tests for `backend: cloudflare` and the nested `cloudflare` block
- [ ] add a failing store test proving backend dispatch reaches the Cloudflare constructor
- [ ] implement the config and dispatch changes until the tests pass

Completion criteria:

- the user can select `cloudflare` as a backend
- the simulator can construct a Cloudflare-backed participant session from config

### Phase 3: Implement `CloudflareSession` start and close

Goal:

- create a real remote participant backend with correct lifecycle semantics

Recommended `CloudflareSession` fields:

- `launch_spec: ParticipantLaunchSpec`
- `cloudflare_config: CloudflareConfig`
- `log_sender: UnboundedSender<ParticipantLogMessage>`
- `api: CloudflareWorkerClient`
- `session_id: Option<String>`
- `cached_state: ParticipantState`
- `termination_task: Option<JoinHandle<DriverTermination>>`
- `termination_rx: watch::Receiver<Option<DriverTermination>>`
- `closing: bool`
- auth dependencies needed to lazily obtain a cookie for Hyper Core

`start()` should:

- prepare the create-session request from `ParticipantLaunchSpec`
- fetch or reuse a `hyper_session` cookie for Hyper Core if needed
- call the worker create endpoint
- store `sessionId`
- initialize `cached_state` from the worker response
- forward worker log entries into the participant log channel
- start the background health poll used by `wait_for_termination()`

`close()` should:

- call the worker close endpoint if a session exists
- stop the termination poll
- clear local session state
- be idempotent

TDD steps:

- [ ] add a failing integration test against a mock worker for successful start
- [ ] add a failing integration test for close-after-start
- [ ] add a failing test for Hyper Core cookie injection into the create request
- [ ] implement `start()` and `close()` until those tests pass

Completion criteria:

- a Cloudflare participant can be started and closed through the shared runtime
- no Cloudflare identifiers leak above the driver

### Phase 4: Implement command handling, cached state, and termination polling

Goal:

- reach full runtime compatibility with the command/state contract

Implementation notes:

- map every `ParticipantMessage` variant to the worker command API
- update `cached_state` from worker command responses
- make `refresh_state()` return the cached authoritative state from the last worker response
- reserve explicit worker state calls for:
  - termination polling
  - recovery or debugging paths

Recommended command semantics:

- `Join` -> worker join command
- `Leave` -> worker leave command, keep the backend session alive
- `ToggleAudio` -> worker toggle audio command
- `ToggleVideo` -> worker toggle video command
- `ToggleScreenshare` -> worker toggle screenshare command
- `SetNoiseSuppression` -> worker set noise suppression command
- `SetWebcamResolutions` -> worker set webcam resolution command
- `ToggleBackgroundBlur` -> worker toggle blur command

Termination handling:

- spawn a background polling task after `start()`
- hit the worker state endpoint at a low frequency
- if the worker reports the session missing, closed, or failed, send a `DriverTermination`
- stop the poll cleanly during intentional close

TDD steps:

- [ ] add one failing test per command mapping
- [ ] add a failing test proving `refresh_state()` reflects command responses without extra network calls
- [ ] add a failing test for unexpected termination on worker `404` or equivalent closed-session signal
- [ ] implement command handling and polling until the tests pass

Completion criteria:

- `CloudflareSession` supports the full `ParticipantMessage` surface
- runtime state remains accurate after start and every command
- unexpected worker-side session loss reaches `wait_for_termination()`

### Phase 5: Add TUI and UX handling for backend-specific limitations

Goal:

- keep the UI understandable without overcomplicating it

Recommended behavior:

- allow all existing participant controls to remain visible
- when the backend cannot honor a local-only setting, surface that through logs and resulting state
- keep the TUI layout stable in the first iteration

Specific decisions to encode:

- `headless` is ignored for Cloudflare because the backend is always headless
- local fake-media file and URL selections are ignored for Cloudflare
- transport should normalize to `WebRTC`; if the user configured `WebTransport`, log the normalization and reflect `WebRTC` in state

Optional follow-up, not required for the first implementation:

- annotate unsupported fields in the TUI when `backend == cloudflare`

TDD steps:

- [ ] add a failing test for transport normalization
- [ ] add a failing test for ignored fake-media settings producing a log entry
- [ ] implement the minimal UX behavior needed for those tests

Completion criteria:

- backend-specific behavior is visible and predictable
- the UI does not need backend-specific branching for every participant command

### Phase 6: Validate with unit, integration, and manual tests

Goal:

- verify the backend works without a real Cloudflare dependency in automated tests

Automated tests:

- add unit tests close to the Cloudflare mapping and config code
- add integration tests in `browser/tests/` with a mock HTTP server such as `wiremock`
- exercise the shared runtime with a real `CloudflareSession` against mocked worker responses

Suggested automated scenarios:

- start success for Hyper Core
- start success for Hyper Lite
- command success for every `ParticipantMessage` variant
- command failure logging without crashing the runtime
- close after leave
- unexpected termination detection
- config parsing and backend dispatch

Manual validation:

- `just test`
- `just clippy`
- run the worker locally and point the simulator at it through the new Cloudflare config block
- join a Hyper Core room
- join a Hyper Lite room
- exercise audio, video, screenshare, noise suppression, resolution, blur, leave, and close

Completion criteria:

- automated tests cover the driver contract
- manual smoke tests pass against a real worker

## Recommended File Changes

Rust workspace:

- `Cargo.toml`
- `browser/Cargo.toml`
- `config/src/client_config.rs`
- `config/src/lib.rs`
- `config/src/default-config.yaml`
- `browser/src/participant/mod.rs`
- `browser/src/participant/shared/store.rs`
- `browser/src/participant/cloudflare/mod.rs`
- `tui/src/tui/components/browser_start.rs`

New files and directories:

- `cloudflare-worker-client/Cargo.toml`
- `cloudflare-worker-client/build.rs`
- `cloudflare-worker-client/openapi/cloudflare-browser-simulator.json`
- `cloudflare-worker-client/src/generated.rs`
- `cloudflare-worker-client/src/lib.rs`
- `cloudflare-worker-client/src/client.rs`
- `browser/tests/cloudflare_driver.rs`

## Risks And Mitigations

### Risk: a separate driver crate causes a dependency cycle

Mitigation:

- keep the generated client in a new crate
- keep the actual `CloudflareSession` in `client-simulator-browser`

### Risk: double round-trips for every command

Mitigation:

- use create and command responses as authoritative state updates
- do not immediately re-fetch state after every command just because the runtime has a `refresh_state()` hook

### Risk: Cloudflare session lifetime is shorter than local Chromium sessions

Mitigation:

- surface the configured keep-alive limits clearly in config and logs
- rely on the termination poller so the runtime reflects worker-side expiry promptly

### Risk: fake-media parity becomes a scope trap

Mitigation:

- explicitly ship v1 with synthetic worker media only
- keep local fake-media handling local-only

## Recommended First Patch Set

The first implementation PR in this repo should do only this:

1. add the generated-client crate
2. add Cloudflare config and backend selection
3. add the `CloudflareSession` skeleton with start and close only
4. add mock-worker integration tests for start and close

That gives a reviewable base. Command parity and polish can land immediately after.
