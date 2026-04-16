# Browser Driver Requirements

This document describes the functionality a browser driver must implement to plug into the `browser` crate today.

There are two useful levels to think about:

- The hard contract required by the shared participant runtime.
- The extra capabilities required to match the current local Chromium backend feature-for-feature.

The shared runtime is intentionally command/state based. A new driver does not need to expose a generic `Page` or `Browser` API; it needs to execute participant commands and report participant state. See [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:34-41` and [`2026-04-15-generic-browser-driver.md`](../2026-04-15-generic-browser-driver.md) `:19-20`.

## 1. Hard Runtime Contract

Any driver plugged into `spawn_session()` must implement `ParticipantDriverSession`. See [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:145-153` and [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:34-41`.

Required methods:

| Method                   | What it must do                                                                                                                                                                                                                                                                 | References                                                                                                                                                                                                                   |
|--------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `participant_name()`     | Return the stable participant identity used in state/logging.                                                                                                                                                                                                                   | [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:54-57`, `:159-160`                                                                                                             |
| `start()`                | Bring the participant session up. In practice this means launch/connect to the backend, navigate/open the target session, perform any auth/setup, and have the participant joined before returning. The runtime treats start failures as fatal and immediately calls `close()`. | [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:59-76`; local behavior in [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:129-159` |
| `handle_command()`       | Execute participant actions after startup. The runtime calls this for all runtime commands except `Close`.                                                                                                                                                                      | [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:118-140`                                                                                                                       |
| `refresh_state()`        | Return the latest `ParticipantState`. The runtime then publishes that to watchers.                                                                                                                                                                                              | [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:149-174`                                                                                                                       |
| `close()`                | Stop the participant and clean up backend resources. The runtime calls this on explicit close, on channel teardown, and after startup failure.                                                                                                                                  | [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:66-72`, `:107-127`                                                                                                             |
| `wait_for_termination()` | Block until the backend dies unexpectedly and return a `DriverTermination`. This is how the runtime notices browser crashes/disconnects.                                                                                                                                        | [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:87-106`                                                                                                                        |

Important runtime-owned details:

- `running` and `username` are effectively runtime-owned. The runtime sets them on startup and overwrites them after every successful state refresh. See [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:54-57`, `:158-163`.
- When the runtime stops, it forces `running = false`, `joined = false`, and `screenshare_activated = false`. See [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:176-183`.
- `ParticipantMessage::Close` is part of the external protocol, but it is handled by the runtime by calling `close()` directly. It is not normally forwarded to `handle_command()`. See [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:118-128`.

## 2. Command Surface The Driver Must Support

The stable command protocol is `ParticipantMessage`. See [`browser/src/participant/shared/messages.rs`](../browser/src/participant/shared/messages.rs) `:7-18`.

The current command set is:

| Command                                  | Meaning                                                                            | Where it is issued                                                                                                                                                                             |
|------------------------------------------|------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `Join`                                   | Join or re-join the session.                                                       | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:202-215`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:257-262`             |
| `Leave`                                  | Leave the session without destroying the participant.                              | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:238-240`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:249-255`             |
| `ToggleAudio`                            | Toggle microphone mute state.                                                      | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:242-244`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:264-270`             |
| `ToggleVideo`                            | Toggle camera on/off.                                                              | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:246-248`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:272-277`             |
| `ToggleScreenshare`                      | Toggle screen sharing on/off.                                                      | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:250-256`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:279-284`             |
| `SetNoiseSuppression(NoiseSuppression)`  | Set the noise suppression model.                                                   | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:258-260`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:160-166`, `:187-195` |
| `SetWebcamResolutions(WebcamResolution)` | Set outgoing camera resolution. The enum variant name is pluralized in code today. | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:262-268`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:170-176`, `:209-217` |
| `ToggleBackgroundBlur`                   | Toggle background blur.                                                            | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:270-272`, [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:294-299`             |

The `Participant` handle will only forward most commands when the participant is still running and currently joined. `Join` is the exception and is allowed as a rejoin path. See [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:217-235`.

## 3. State The Driver Must Report

The shared observable state is `ParticipantState`. See [`browser/src/participant/shared/state.rs`](../browser/src/participant/shared/state.rs) `:7-19`.

For full compatibility with the current TUI, `refresh_state()` needs to keep these fields up to date:

| Field                   | Why it matters                                                                                        | References                                                                                                                                                                                                                            |
|-------------------------|-------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `joined`                | Determines whether the participant is in the session and gates most outgoing commands.                | [`browser/src/participant/mod.rs`](../browser/src/participant/mod.rs) `:208-210`, `:227-229`; table display in [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:333-345`, `:364-385`           |
| `muted`                 | Drives mute/unmute status in the participant table.                                                   | [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:338-339`, `:366-367`                                                                                                                          |
| `video_activated`       | Drives camera status in the participant table.                                                        | [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:339-340`, `:367-368`                                                                                                                          |
| `screenshare_activated` | Drives screen share status in the participant table and is reset on stop.                             | [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:340-341`, `:368-369`; runtime reset in [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:176-183` |
| `noise_suppression`     | Used for the selection dialog and status display.                                                     | [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:160-166`, `:341-342`, `:369-370`                                                                                                              |
| `transport_mode`        | Displayed in the participant table. There is currently no runtime command to change it after startup. | [`browser/src/participant/shared/state.rs`](../browser/src/participant/shared/state.rs) `:15`; table display in [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:342-343`, `:370-371`          |
| `webcam_resolution`     | Used for the selection dialog and status display.                                                     | [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:170-176`, `:343-344`, `:371-372`                                                                                                              |
| `background_blur`       | Displayed in the participant table and toggled at runtime.                                            | [`tui/src/tui/components/participants.rs`](../tui/src/tui/components/participants.rs) `:344-345`, `:372-384`                                                                                                                          |

Notes:

- `running` is maintained by the runtime, not by the driver contract. See [`browser/src/participant/shared/runtime.rs`](../browser/src/participant/shared/runtime.rs) `:54-57`, `:158-163`, `:176-183`.
- The current lite frontend returns defaults or no-ops for unsupported advanced controls such as noise suppression, transport inspection, resolution changes, and blur. See [`browser/src/participant/local/lite.rs`](../browser/src/participant/local/lite.rs) `:153-175`, `:193-223`. A new driver should at least preserve consistent behavior if a frontend/backend cannot support a setting.

## 4. Launch-Time Inputs The Driver Is Expected To Honor

Drivers are created from `ParticipantLaunchSpec`, which contains the participant identity, target URL, resolved frontend kind, and desired initial settings. See [`browser/src/participant/shared/spec.rs`](../browser/src/participant/shared/spec.rs) `:26-77`.

The initial settings payload currently includes:

- `audio_enabled`
- `video_enabled`
- `screenshare_enabled`
- `noise_suppression`
- `transport`
- `resolution`
- `blur`

See [`browser/src/participant/shared/spec.rs`](../browser/src/participant/shared/spec.rs) `:27-35`, `:37-49`.

Those settings come from top-level config/TUI controls. See [`config/src/lib.rs`](../config/src/lib.rs) `:46-74` and [`tui/src/tui/components/browser_start.rs`](../tui/src/tui/components/browser_start.rs) `:571-590`.

For parity with the current local backend, `start()` should apply those launch settings automatically:

- Hyper Core applies noise suppression, blur, camera resolution, and transport before join, then applies audio/video in the lobby, and applies initial screenshare after joining. See [`browser/src/participant/local/core.rs`](../browser/src/participant/local/core.rs) `:143-171`.
- Hyper Lite currently only applies audio/video/screenshare and ignores advanced media settings. See [`browser/src/participant/local/lite.rs`](../browser/src/participant/local/lite.rs) `:90-106`, `:153-175`.

There is currently no runtime command for:

- changing transport mode after startup,
- changing the session URL after startup,
- changing fake media after startup,
- changing participant name after startup.

Those are launch-time concerns today.

## 5. Extra Capabilities Needed For Full Local Chromium Parity

If the goal is not just "implement the runtime trait" but "replace the current local browser backend without losing features", the driver also needs these capabilities.

### 5.1 Browser/session lifecycle

- Launch a browser process or connect to an equivalent remote browser session. See [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:295-356`.
- Open the target session URL and retry page creation on transient failures. See [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:412-482`.
- Support headed/headless startup and a browser user data directory. See [`config/src/browser_config.rs`](../config/src/browser_config.rs) `:35-49` and [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:339-349`.
- Shut down cleanly, including leaving the session if possible, closing the page/browser, and killing the browser on failed startup. See [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:154-156`, `:168-222`.

### 5.2 Authentication / identity setup

- Hyper Core currently requires a `hyper_session` cookie to be present in the browser before navigation. The local driver either reuses a stored cookie or fetches a new guest cookie and sets the display name through the HTTP auth API. See [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:263-279`, [`browser/src/auth.rs`](../browser/src/auth.rs) `:61-85`, `:253-349`, and [`browser/src/participant/local/core.rs`](../browser/src/participant/local/core.rs) `:57-80`.
- Hyper Core then fills the participant name in the join form and clicks join. See [`browser/src/participant/local/core.rs`](../browser/src/participant/local/core.rs) `:90-140`.
- Hyper Lite does not use the cookie path and joins by clicking the join button directly. See [`browser/src/participant/local/lite.rs`](../browser/src/participant/local/lite.rs) `:49-88`.

### 5.3 Frontend-specific control hooks

The current implementation has two frontend-specific automation layers:

- Hyper Core, resolved for non-`/m` URLs. See [`browser/src/participant/shared/spec.rs`](../browser/src/participant/shared/spec.rs) `:9-23`.
- Hyper Lite, resolved for `/m` URLs. See [`browser/src/participant/shared/spec.rs`](../browser/src/participant/shared/spec.rs) `:15-23`.

For Hyper Core parity, the driver needs hooks for:

- join button, leave button, mute button, video button, screen share button, name input selectors, or equivalent control channels; see [`browser/src/participant/local/selectors.rs`](../browser/src/participant/local/selectors.rs) `:1-20`
- JS getters/setters for noise suppression, blur, camera resolution, and force-WebRTC transport; see [`browser/src/participant/local/commands.rs`](../browser/src/participant/local/commands.rs) `:80-124`
- state scraping for mute/video/screenshare button state and advanced media settings; see [`browser/src/participant/local/core.rs`](../browser/src/participant/local/core.rs) `:271-317`

For Hyper Lite parity, the driver needs the join/leave/mute/video/screenshare controls and state scraping, but the advanced settings can remain unsupported no-ops as they are today. See [`browser/src/participant/local/lite.rs`](../browser/src/participant/local/lite.rs) `:119-175`, `:193-223`.

### 5.4 Fake media / media injection

The current local backend supports fake media injection at browser startup:

- no fake media,
- Chromium builtin fake devices,
- a file or URL converted into Chromium-compatible fake audio/video inputs.

See [`config/src/media/mod.rs`](../config/src/media/mod.rs) `:8-59`, [`config/src/media/custom_fake_media.rs`](../config/src/media/custom_fake_media.rs) `:45-105`, and [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:298-337`.

This is not part of `ParticipantMessage`, but it is part of the browser driver's current functional surface because the spawn UI lets the user choose fake media before launch. See [`config/src/default-config.yaml`](../config/src/default-config.yaml) `:1-33` and [`tui/src/tui/components/browser_start.rs`](../tui/src/tui/components/browser_start.rs) `:593-612`.

### 5.5 Unexpected termination detection

The runtime expects the driver to notice when the browser dies unexpectedly and surface that through `wait_for_termination()`.

The current local backend does this in two ways:

- browser handler errors (`ResetWithoutClosingHandshake`), and
- detached-target events from Chromium.

See [`browser/src/participant/local/session.rs`](../browser/src/participant/local/session.rs) `:358-410`.

## 6. Short Checklist

If you are implementing a new driver, this is the minimum checklist:

- Implement `ParticipantDriverSession`.
- Make `start()` end with a joined participant session.
- Support `Join`, `Leave`, `ToggleAudio`, `ToggleVideo`, `ToggleScreenshare`, `SetNoiseSuppression`, `SetWebcamResolutions`, and `ToggleBackgroundBlur`.
- Return accurate `ParticipantState` for the fields the TUI displays and edits.
- Implement `close()` and unexpected-termination reporting.
- Honor launch-time settings from `ParticipantLaunchSpec`.

If you need drop-in parity with the current local Chromium backend, also implement:

- Hyper Core auth/cookie setup,
- Hyper Core and Hyper Lite frontend control flows,
- fake media injection,
- headless/headed browser startup,
- browser crash/disconnect detection.
