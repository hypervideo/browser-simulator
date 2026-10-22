use crate::participant::shared::{
    browser_log::{
        console_level,
        emit_browser_log_batch,
        BrowserLogEntry,
        BrowserLogSource,
    },
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    DriverTermination,
    ParticipantDriverSession,
    ParticipantLaunchSpec,
    ParticipantState,
    ResolvedFrontendKind,
};
use client_simulator_config::{
    media::FakeMedia,
    CloudflareConfig,
    NoiseSuppression,
    TransportMode,
    VideoConstraint,
    VideoMaxConcurrentTracks,
};
use cloudflare_worker_client::{
    types,
    CloudflareWorkerClient,
};
use eyre::{
    bail,
    eyre,
    Context as _,
    Result,
};
use futures::{
    future::BoxFuture,
    FutureExt as _,
};
use std::{
    sync::{
        Arc,
        Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::{
        oneshot,
        watch,
    },
    task::JoinHandle,
    time::MissedTickBehavior,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CloudflareLaunchOptions {
    headless: bool,
    fake_media: FakeMedia,
    browser_logs: bool,
}

impl From<&client_simulator_config::Config> for CloudflareLaunchOptions {
    fn from(config: &client_simulator_config::Config) -> Self {
        Self {
            headless: config.headless,
            fake_media: config.fake_media(),
            browser_logs: config.browser_logs,
        }
    }
}

pub(super) struct CloudflareSession {
    launch_spec: ParticipantLaunchSpec,
    launch_options: CloudflareLaunchOptions,
    cloudflare_config: CloudflareConfig,
    session_id: Option<String>,
    cached_state: Arc<Mutex<ParticipantState>>,
    termination_tx: watch::Sender<Option<DriverTermination>>,
    termination_rx: watch::Receiver<Option<DriverTermination>>,
    poller_shutdown_tx: Option<oneshot::Sender<()>>,
    poller_task: Option<JoinHandle<()>>,
}

fn emit_log_message(participant_name: &str, level: &str, message: impl ToString) {
    ParticipantLogMessage::new(level, participant_name, message).write();
}

fn forward_worker_entries(participant_name: &str, entries: &[types::AutomationLogEntry]) {
    for entry in entries {
        emit_log_message(
            participant_name,
            "debug",
            format!("worker {} {}", entry.at.to_rfc3339(), entry.step),
        );
    }
}

fn forward_worker_browser_entries(participant_name: &str, entries: &[types::BrowserLogEntry]) {
    let entries = entries
        .iter()
        .map(|entry| map_worker_browser_entry(participant_name, entry))
        .collect();
    emit_browser_log_batch(participant_name, entries);
}

fn map_worker_browser_entry(participant_name: &str, entry: &types::BrowserLogEntry) -> BrowserLogEntry {
    let source = match entry.source {
        types::BrowserLogEntrySource::Console => BrowserLogSource::Console,
        types::BrowserLogEntrySource::Exception => BrowserLogSource::Exception,
        types::BrowserLogEntrySource::Browser => BrowserLogSource::Browser,
    };

    BrowserLogEntry::new(
        participant_name,
        source,
        console_level(&entry.level),
        entry.message.clone(),
    )
}

fn store_cached_state(
    participant_name: &str,
    cached_state: &Arc<Mutex<ParticipantState>>,
    state: &types::ParticipantState,
) {
    *cached_state.lock().unwrap() = map_state(participant_name, state);
}

impl CloudflareSession {
    pub(super) fn new(
        launch_spec: ParticipantLaunchSpec,
        launch_options: CloudflareLaunchOptions,
        cloudflare_config: CloudflareConfig,
    ) -> Self {
        Self::build(launch_spec, launch_options, cloudflare_config, true)
    }

    fn build(
        launch_spec: ParticipantLaunchSpec,
        launch_options: CloudflareLaunchOptions,
        cloudflare_config: CloudflareConfig,
        _track_spawn: bool,
    ) -> Self {
        #[cfg(test)]
        {
            if _track_spawn {
                spawned_participants_for_test()
                    .lock()
                    .unwrap()
                    .push(launch_spec.username.clone());
            }
        }

        let (termination_tx, termination_rx) = watch::channel(None);

        Self {
            cached_state: Arc::new(Mutex::new(ParticipantState {
                username: launch_spec.username.clone(),
                ..Default::default()
            })),
            launch_spec,
            launch_options,
            cloudflare_config,
            session_id: None,
            termination_tx,
            termination_rx,
            poller_shutdown_tx: None,
            poller_task: None,
        }
    }

    #[cfg(test)]
    fn new_for_test(
        launch_spec: ParticipantLaunchSpec,
        launch_options: CloudflareLaunchOptions,
        cloudflare_config: CloudflareConfig,
    ) -> Self {
        Self::build(launch_spec, launch_options, cloudflare_config, false)
    }

    fn log_message(&self, level: &str, message: impl ToString) {
        emit_log_message(&self.launch_spec.username, level, message);
    }

    fn worker_client(&self) -> Result<CloudflareWorkerClient> {
        CloudflareWorkerClient::new(
            self.cloudflare_config.base_url.as_ref(),
            Duration::from_secs(self.cloudflare_config.request_timeout_seconds),
        )
        .wrap_err("Failed to construct Cloudflare worker client")
    }

    fn log_worker_entries(&self, entries: &[types::AutomationLogEntry]) {
        forward_worker_entries(&self.launch_spec.username, entries);
    }

    fn log_worker_browser_entries(&self, entries: &[types::BrowserLogEntry]) {
        forward_worker_browser_entries(&self.launch_spec.username, entries);
    }

    fn log_backend_limitations(&self) {
        if !self.launch_options.headless {
            self.log_message(
                "warn",
                "Cloudflare backend ignores headless=false because worker sessions are always headless",
            );
        }

        if let FakeMedia::FileOrUrl(source) = &self.launch_options.fake_media {
            self.log_message(
                "warn",
                format!(
                    "Cloudflare backend ignores local fake media source `{source}` and will use worker-provided media instead"
                ),
            );
        }
    }

    fn normalized_settings(&self) -> crate::participant::shared::ParticipantSettings {
        let mut settings = self.launch_spec.settings.clone();

        if settings.transport != TransportMode::WebRTC {
            self.log_message(
                "warn",
                format!(
                    "Cloudflare backend only supports WebRTC transport; normalizing configured {} transport to WebRTC",
                    settings.transport
                ),
            );
            settings.transport = TransportMode::WebRTC;
        }

        match settings.noise_suppression {
            NoiseSuppression::Disabled => {}
            NoiseSuppression::HyperVoice => {
                self.log_message(
                    "warn",
                    format!(
                        "Cloudflare backend only supports noise suppression none; normalizing configured {} to none",
                        settings.noise_suppression
                    ),
                );
                settings.noise_suppression = NoiseSuppression::Disabled;
            }
        }

        settings
    }

    async fn build_create_request(&mut self) -> Result<types::SessionCreateRequest> {
        self.log_backend_limitations();
        let normalized_settings = self.normalized_settings();

        Ok(types::SessionCreateRequest {
            browser_logs: Some(self.launch_options.browser_logs),
            debug: Some(self.cloudflare_config.debug),
            display_name: types::SessionCreateRequestDisplayName::try_from(self.launch_spec.username.clone())
                .map_err(|error| eyre!("Invalid Cloudflare display name: {error}"))?,
            frontend_kind: map_frontend_kind(self.launch_spec.frontend_kind),
            navigation_timeout_ms: Some(self.cloudflare_config.navigation_timeout_ms as f64),
            room_url: self.launch_spec.session_url.to_string(),
            selector_timeout_ms: Some(self.cloudflare_config.selector_timeout_ms as f64),
            session_timeout_ms: Some(self.cloudflare_config.session_timeout_ms as f64),
            settings: map_settings(&normalized_settings),
        })
    }

    fn command_request(message: ParticipantMessage) -> Result<types::SessionCommandRequest> {
        Ok(match message {
            ParticipantMessage::Join => types::SessionCommandRequest::Join,
            ParticipantMessage::Leave => types::SessionCommandRequest::Leave,
            ParticipantMessage::Close => types::SessionCommandRequest::Leave,
            ParticipantMessage::ToggleAudio => types::SessionCommandRequest::ToggleAudio,
            ParticipantMessage::ToggleVideo => types::SessionCommandRequest::ToggleVideo,
            ParticipantMessage::ToggleScreenshare => types::SessionCommandRequest::ToggleScreenshare,
            ParticipantMessage::ToggleAutoGainControl => types::SessionCommandRequest::ToggleAutoGainControl,
            ParticipantMessage::SetNoiseSuppression(value) => types::SessionCommandRequest::SetNoiseSuppression {
                noise_suppression: map_command_noise_suppression(value)?,
            },
            ParticipantMessage::SetVideoConstraintPublishWebcam(value) => {
                types::SessionCommandRequest::SetVideoConstraintPublishWebcam {
                    video_constraint_publish_webcam: map_command_video_constraint_publish_webcam(value),
                }
            }
            ParticipantMessage::SetVideoConstraintSubscribe(value) => {
                types::SessionCommandRequest::SetVideoConstraintSubscribe {
                    video_constraint_subscribe: map_command_video_constraint_subscribe(value),
                }
            }
            ParticipantMessage::SetVideoMaxConcurrentTracks(value) => {
                types::SessionCommandRequest::SetVideoMaxConcurrentTracks {
                    video_max_concurrent_tracks: value.limit().map(|value| value as u64),
                }
            }
            ParticipantMessage::ToggleBackgroundBlur => types::SessionCommandRequest::ToggleBackgroundBlur,
        })
    }

    fn cached_state(&self) -> ParticipantState {
        self.cached_state.lock().unwrap().clone()
    }

    fn update_cached_state(&self, state: &types::ParticipantState) {
        store_cached_state(&self.launch_spec.username, &self.cached_state, state);
    }

    fn effective_health_poll_interval(&self) -> Duration {
        let configured_ms = self.cloudflare_config.health_poll_interval_ms.max(1);
        let keep_alive_budget_ms = self.cloudflare_config.session_timeout_ms.saturating_div(2).max(1);
        let effective_ms = configured_ms.min(keep_alive_budget_ms);

        if effective_ms != configured_ms {
            self.log_message(
                "warn",
                format!(
                    "Cloudflare health poll interval {}ms exceeds the safe keep-alive window for a {}ms Browser Rendering keep_alive timeout; clamping to {}ms",
                    configured_ms, self.cloudflare_config.session_timeout_ms, effective_ms
                ),
            );
        }

        Duration::from_millis(effective_ms)
    }

    async fn stop_termination_poller(&mut self) {
        if let Some(shutdown_tx) = self.poller_shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(task) = self.poller_task.take() {
            let _ = task.await;
        }
    }

    fn start_termination_poller(&mut self, session_id: String) -> Result<()> {
        let client = self.worker_client()?;
        let poll_interval = self.effective_health_poll_interval();
        let cached_state = Arc::clone(&self.cached_state);
        let participant_name = self.launch_spec.username.clone();
        let termination_tx = self.termination_tx.clone();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    _ = interval.tick() => {
                        match client.keep_alive_session(&session_id).await {
                            Ok(response) => {
                                forward_worker_entries(&participant_name, &response.log);
                                forward_worker_browser_entries(&participant_name, &response.browser_log);

                                store_cached_state(&participant_name, &cached_state, &response.state);

                                if !response.state.running {
                                    let _ = termination_tx.send(Some(DriverTermination::new(
                                        "warn",
                                        format!(
                                            "Cloudflare worker session {session_id} is no longer running"
                                        ),
                                    )));
                                    break;
                                }
                            }
                            Err(err) => {
                                let _ = termination_tx.send(Some(DriverTermination::new(
                                    "warn",
                                    format!(
                                        "Cloudflare worker session {session_id} terminated unexpectedly: {err}"
                                    ),
                                )));
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.poller_shutdown_tx = Some(shutdown_tx);
        self.poller_task = Some(task);

        Ok(())
    }

    async fn start_inner(&mut self) -> Result<()> {
        if self.session_id.is_some() {
            bail!("Cloudflare session already started");
        }

        self.log_message(
            "info",
            format!(
                "Creating Cloudflare worker session via {}",
                self.cloudflare_config.base_url
            ),
        );

        let request = self.build_create_request().await?;
        let response = self.worker_client()?.create_session(&request).await?;
        self.log_worker_entries(&response.log);
        if let Some(entries) = response.browser_log.as_deref() {
            self.log_worker_browser_entries(entries);
        } else if self.launch_options.browser_logs {
            self.log_message("debug", "worker does not report browser logs");
        }

        self.termination_tx.send_replace(None);
        self.update_cached_state(&response.state);
        self.session_id = Some(response.session_id.clone());
        self.start_termination_poller(response.session_id.clone())?;

        self.log_message(
            "info",
            format!("Created Cloudflare worker session {}", response.session_id),
        );

        Ok(())
    }

    async fn close_inner(&mut self) -> Result<()> {
        self.stop_termination_poller().await;

        let Some(session_id) = self.session_id.clone() else {
            self.log_message("debug", "Cloudflare worker session already closed");
            return Ok(());
        };

        self.log_message("info", format!("Closing Cloudflare worker session {session_id}"));

        let response = self.worker_client()?.close_session(&session_id).await?;
        self.log_worker_entries(&response.log);
        self.log_worker_browser_entries(&response.browser_log);
        self.session_id = None;
        self.termination_tx.send_replace(None);
        {
            let mut cached_state = self.cached_state.lock().unwrap();
            cached_state.running = false;
            cached_state.joined = false;
            cached_state.screenshare_activated = false;
        }

        self.log_message("info", format!("Closed Cloudflare worker session {session_id}"));

        Ok(())
    }

    async fn handle_command_inner(&mut self, message: ParticipantMessage) -> Result<()> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| eyre!("Cloudflare session is not started"))?;
        let request = Self::command_request(message)?;
        let response = self.worker_client()?.command_session(&session_id, &request).await?;
        self.log_worker_entries(&response.log);
        self.log_worker_browser_entries(&response.browser_log);
        self.update_cached_state(&response.state);
        Ok(())
    }

    async fn wait_for_termination_inner(&mut self) -> DriverTermination {
        loop {
            if let Some(termination) = self.termination_rx.borrow().clone() {
                return termination;
            }

            if self.termination_rx.changed().await.is_err() {
                return DriverTermination::new("warn", "cloudflare driver termination channel closed");
            }
        }
    }
}

impl ParticipantDriverSession for CloudflareSession {
    fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    fn start(&mut self) -> BoxFuture<'_, Result<()>> {
        self.start_inner().boxed()
    }

    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>> {
        self.handle_command_inner(message).boxed()
    }

    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>> {
        async move { Ok(self.cached_state()) }.boxed()
    }

    fn close(&mut self) -> BoxFuture<'_, Result<()>> {
        self.close_inner().boxed()
    }

    fn wait_for_termination(&mut self) -> BoxFuture<'_, DriverTermination> {
        self.wait_for_termination_inner().boxed()
    }
}

fn map_frontend_kind(frontend_kind: ResolvedFrontendKind) -> types::SessionCreateRequestFrontendKind {
    match frontend_kind {
        ResolvedFrontendKind::HyperCore => types::SessionCreateRequestFrontendKind::HyperCore,
        ResolvedFrontendKind::HyperLite => types::SessionCreateRequestFrontendKind::HyperLite,
    }
}

/// Maps the settings that [`CloudflareSession::normalized_settings`] returns onto the worker's.
///
/// The worker's schema lacks `hyper-voice`, which `normalized_settings` replaces with `none`, so it maps to `none`.
/// The schema also has no way to leave a setting to hyper.video, so a setting that the participant leaves to
/// hyper.video sends hyper.video's default, and the track limit no limit, `null`.
fn map_settings(settings: &crate::participant::shared::ParticipantSettings) -> types::ParticipantSettings {
    types::ParticipantSettings {
        audio_enabled: settings.audio_enabled,
        auto_gain_control: settings.auto_gain_control.own().unwrap_or(true),
        blur: settings.blur.own().unwrap_or(false),
        noise_suppression: match settings.noise_suppression {
            NoiseSuppression::Disabled | NoiseSuppression::HyperVoice => {
                types::ParticipantSettingsNoiseSuppression::None
            }
        },
        screenshare_enabled: settings.screenshare_enabled,
        transport: match settings.transport {
            client_simulator_config::TransportMode::WebRTC => types::ParticipantSettingsTransport::Webrtc,
            client_simulator_config::TransportMode::WebTransport => types::ParticipantSettingsTransport::Webtransport,
        },
        video_constraint_publish_webcam: map_video_constraint_publish_webcam(
            settings
                .video_constraint_publish_webcam
                .own()
                .unwrap_or(VideoConstraint::None),
        ),
        video_constraint_subscribe: map_video_constraint_subscribe(
            settings
                .video_constraint_subscribe
                .own()
                .unwrap_or(VideoConstraint::None),
        ),
        video_max_concurrent_tracks: settings
            .video_max_concurrent_tracks
            .own()
            .and_then(VideoMaxConcurrentTracks::limit)
            .map(|value| value as u64),
        video_enabled: settings.video_enabled,
    }
}

fn map_state(participant_name: &str, state: &types::ParticipantState) -> ParticipantState {
    ParticipantState {
        username: String::new(),
        running: state.running,
        warning: None,
        joined: state.joined,
        muted: state.muted,
        video_activated: state.video_activated,
        auto_gain_control: state.auto_gain_control,
        noise_suppression: map_state_noise_suppression(participant_name, state.noise_suppression),
        transport_mode: match state.transport_mode {
            types::ParticipantStateTransportMode::Webrtc => client_simulator_config::TransportMode::WebRTC,
            types::ParticipantStateTransportMode::Webtransport => client_simulator_config::TransportMode::WebTransport,
        },
        video_constraint_publish_webcam: map_state_video_constraint_publish_webcam(
            state.video_constraint_publish_webcam,
        ),
        video_constraint_subscribe: map_state_video_constraint_subscribe(state.video_constraint_subscribe),
        video_max_concurrent_tracks: state
            .video_max_concurrent_tracks
            .map_or(VideoMaxConcurrentTracks::Unlimited, |value| {
                VideoMaxConcurrentTracks::Limit(value as usize)
            }),
        background_blur: state.background_blur,
        screenshare_activated: state.screenshare_activated,
    }
}

/// Maps a runtime noise suppression change onto the worker's, refusing a value that the worker can't select.
///
/// The worker's schema takes `none` and 16 engines that hyper.video no longer offers, but not `hyper-voice`, so
/// Cloudflare participants run without noise suppression until the worker takes `hyper-voice`.
fn map_command_noise_suppression(value: NoiseSuppression) -> Result<types::SessionCommandRequestNoiseSuppression> {
    match value {
        NoiseSuppression::Disabled => Ok(types::SessionCommandRequestNoiseSuppression::None),
        NoiseSuppression::HyperVoice => Err(eyre!(
            "Cloudflare backend only supports noise suppression none, not {value}"
        )),
    }
}

/// Maps the worker's reported noise suppression, reporting `none`, with a warning, for an engine that hyper.video no
/// longer offers.
///
/// The backend only requests `none` (see [`map_command_noise_suppression`]), so the worker reports such an engine only
/// if the page runs one that the simulator did not select.
fn map_state_noise_suppression(
    participant_name: &str,
    value: types::ParticipantStateNoiseSuppression,
) -> NoiseSuppression {
    match value {
        types::ParticipantStateNoiseSuppression::None => NoiseSuppression::Disabled,
        types::ParticipantStateNoiseSuppression::Deepfilternet
        | types::ParticipantStateNoiseSuppression::Rnnoise
        | types::ParticipantStateNoiseSuppression::IrisCarthy
        | types::ParticipantStateNoiseSuppression::KrispHigh
        | types::ParticipantStateNoiseSuppression::KrispMedium
        | types::ParticipantStateNoiseSuppression::KrispLow
        | types::ParticipantStateNoiseSuppression::KrispHighWithBvc
        | types::ParticipantStateNoiseSuppression::KrispMediumWithBvc
        | types::ParticipantStateNoiseSuppression::AiCousticsSparrowXxs
        | types::ParticipantStateNoiseSuppression::AiCousticsSparrowXs
        | types::ParticipantStateNoiseSuppression::AiCousticsSparrowS
        | types::ParticipantStateNoiseSuppression::AiCousticsSparrowL
        | types::ParticipantStateNoiseSuppression::AiCousticsSparrowXxs48khz
        | types::ParticipantStateNoiseSuppression::AiCousticsSparrowXs48khz
        | types::ParticipantStateNoiseSuppression::AiCousticsRookS48khz
        | types::ParticipantStateNoiseSuppression::AiCousticsRookL48khz => {
            emit_log_message(
                participant_name,
                "warn",
                format!(
                    "Cloudflare worker reports noise suppression {value}, which hyper.video no longer offers; reporting {}",
                    NoiseSuppression::Disabled
                ),
            );
            NoiseSuppression::Disabled
        }
    }
}

/// Maps the outgoing webcam constraint to the worker's.
///
/// The worker's schema has no 4320p. hyper.video has no resolution level above it, so a 4320p limit leaves every
/// resolution, as `none` does.
fn map_video_constraint_publish_webcam(
    value: client_simulator_config::VideoConstraint,
) -> types::ParticipantSettingsVideoConstraintPublishWebcam {
    match value {
        client_simulator_config::VideoConstraint::None => types::ParticipantSettingsVideoConstraintPublishWebcam::None,
        client_simulator_config::VideoConstraint::P90 => types::ParticipantSettingsVideoConstraintPublishWebcam::X90p,
        client_simulator_config::VideoConstraint::P144 => types::ParticipantSettingsVideoConstraintPublishWebcam::X144p,
        client_simulator_config::VideoConstraint::P240 => types::ParticipantSettingsVideoConstraintPublishWebcam::X240p,
        client_simulator_config::VideoConstraint::P360 => types::ParticipantSettingsVideoConstraintPublishWebcam::X360p,
        client_simulator_config::VideoConstraint::P480 => types::ParticipantSettingsVideoConstraintPublishWebcam::X480p,
        client_simulator_config::VideoConstraint::P720 => types::ParticipantSettingsVideoConstraintPublishWebcam::X720p,
        client_simulator_config::VideoConstraint::P1080 => {
            types::ParticipantSettingsVideoConstraintPublishWebcam::X1080p
        }
        client_simulator_config::VideoConstraint::P1440 => {
            types::ParticipantSettingsVideoConstraintPublishWebcam::X1440p
        }
        client_simulator_config::VideoConstraint::P2160 => {
            types::ParticipantSettingsVideoConstraintPublishWebcam::X2160p
        }
        client_simulator_config::VideoConstraint::P4320 => types::ParticipantSettingsVideoConstraintPublishWebcam::None,
    }
}

/// Maps the incoming video constraint to the worker's, 4320p to `none`, as [`map_video_constraint_publish_webcam`]
/// says.
fn map_video_constraint_subscribe(
    value: client_simulator_config::VideoConstraint,
) -> types::ParticipantSettingsVideoConstraintSubscribe {
    match value {
        client_simulator_config::VideoConstraint::None => types::ParticipantSettingsVideoConstraintSubscribe::None,
        client_simulator_config::VideoConstraint::P90 => types::ParticipantSettingsVideoConstraintSubscribe::X90p,
        client_simulator_config::VideoConstraint::P144 => types::ParticipantSettingsVideoConstraintSubscribe::X144p,
        client_simulator_config::VideoConstraint::P240 => types::ParticipantSettingsVideoConstraintSubscribe::X240p,
        client_simulator_config::VideoConstraint::P360 => types::ParticipantSettingsVideoConstraintSubscribe::X360p,
        client_simulator_config::VideoConstraint::P480 => types::ParticipantSettingsVideoConstraintSubscribe::X480p,
        client_simulator_config::VideoConstraint::P720 => types::ParticipantSettingsVideoConstraintSubscribe::X720p,
        client_simulator_config::VideoConstraint::P1080 => types::ParticipantSettingsVideoConstraintSubscribe::X1080p,
        client_simulator_config::VideoConstraint::P1440 => types::ParticipantSettingsVideoConstraintSubscribe::X1440p,
        client_simulator_config::VideoConstraint::P2160 => types::ParticipantSettingsVideoConstraintSubscribe::X2160p,
        client_simulator_config::VideoConstraint::P4320 => types::ParticipantSettingsVideoConstraintSubscribe::None,
    }
}

fn map_state_video_constraint_publish_webcam(
    value: types::ParticipantStateVideoConstraintPublishWebcam,
) -> client_simulator_config::VideoConstraint {
    match value {
        types::ParticipantStateVideoConstraintPublishWebcam::None => client_simulator_config::VideoConstraint::None,
        types::ParticipantStateVideoConstraintPublishWebcam::X90p => client_simulator_config::VideoConstraint::P90,
        types::ParticipantStateVideoConstraintPublishWebcam::X144p => client_simulator_config::VideoConstraint::P144,
        types::ParticipantStateVideoConstraintPublishWebcam::X240p => client_simulator_config::VideoConstraint::P240,
        types::ParticipantStateVideoConstraintPublishWebcam::X360p => client_simulator_config::VideoConstraint::P360,
        types::ParticipantStateVideoConstraintPublishWebcam::X480p => client_simulator_config::VideoConstraint::P480,
        types::ParticipantStateVideoConstraintPublishWebcam::X720p => client_simulator_config::VideoConstraint::P720,
        types::ParticipantStateVideoConstraintPublishWebcam::X1080p => client_simulator_config::VideoConstraint::P1080,
        types::ParticipantStateVideoConstraintPublishWebcam::X1440p => client_simulator_config::VideoConstraint::P1440,
        types::ParticipantStateVideoConstraintPublishWebcam::X2160p => client_simulator_config::VideoConstraint::P2160,
    }
}

fn map_state_video_constraint_subscribe(
    value: types::ParticipantStateVideoConstraintSubscribe,
) -> client_simulator_config::VideoConstraint {
    match value {
        types::ParticipantStateVideoConstraintSubscribe::None => client_simulator_config::VideoConstraint::None,
        types::ParticipantStateVideoConstraintSubscribe::X90p => client_simulator_config::VideoConstraint::P90,
        types::ParticipantStateVideoConstraintSubscribe::X144p => client_simulator_config::VideoConstraint::P144,
        types::ParticipantStateVideoConstraintSubscribe::X240p => client_simulator_config::VideoConstraint::P240,
        types::ParticipantStateVideoConstraintSubscribe::X360p => client_simulator_config::VideoConstraint::P360,
        types::ParticipantStateVideoConstraintSubscribe::X480p => client_simulator_config::VideoConstraint::P480,
        types::ParticipantStateVideoConstraintSubscribe::X720p => client_simulator_config::VideoConstraint::P720,
        types::ParticipantStateVideoConstraintSubscribe::X1080p => client_simulator_config::VideoConstraint::P1080,
        types::ParticipantStateVideoConstraintSubscribe::X1440p => client_simulator_config::VideoConstraint::P1440,
        types::ParticipantStateVideoConstraintSubscribe::X2160p => client_simulator_config::VideoConstraint::P2160,
    }
}

/// Maps the outgoing webcam constraint to the worker's command, 4320p to `none`, as
/// [`map_video_constraint_publish_webcam`] says.
fn map_command_video_constraint_publish_webcam(
    value: client_simulator_config::VideoConstraint,
) -> types::SessionCommandRequestVideoConstraintPublishWebcam {
    match value {
        client_simulator_config::VideoConstraint::None => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::None
        }
        client_simulator_config::VideoConstraint::P90 => types::SessionCommandRequestVideoConstraintPublishWebcam::X90p,
        client_simulator_config::VideoConstraint::P144 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X144p
        }
        client_simulator_config::VideoConstraint::P240 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X240p
        }
        client_simulator_config::VideoConstraint::P360 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X360p
        }
        client_simulator_config::VideoConstraint::P480 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X480p
        }
        client_simulator_config::VideoConstraint::P720 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X720p
        }
        client_simulator_config::VideoConstraint::P1080 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X1080p
        }
        client_simulator_config::VideoConstraint::P1440 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X1440p
        }
        client_simulator_config::VideoConstraint::P2160 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::X2160p
        }
        client_simulator_config::VideoConstraint::P4320 => {
            types::SessionCommandRequestVideoConstraintPublishWebcam::None
        }
    }
}

/// Maps the incoming video constraint to the worker's command, 4320p to `none`, as
/// [`map_video_constraint_publish_webcam`] says.
fn map_command_video_constraint_subscribe(
    value: client_simulator_config::VideoConstraint,
) -> types::SessionCommandRequestVideoConstraintSubscribe {
    match value {
        client_simulator_config::VideoConstraint::None => types::SessionCommandRequestVideoConstraintSubscribe::None,
        client_simulator_config::VideoConstraint::P90 => types::SessionCommandRequestVideoConstraintSubscribe::X90p,
        client_simulator_config::VideoConstraint::P144 => types::SessionCommandRequestVideoConstraintSubscribe::X144p,
        client_simulator_config::VideoConstraint::P240 => types::SessionCommandRequestVideoConstraintSubscribe::X240p,
        client_simulator_config::VideoConstraint::P360 => types::SessionCommandRequestVideoConstraintSubscribe::X360p,
        client_simulator_config::VideoConstraint::P480 => types::SessionCommandRequestVideoConstraintSubscribe::X480p,
        client_simulator_config::VideoConstraint::P720 => types::SessionCommandRequestVideoConstraintSubscribe::X720p,
        client_simulator_config::VideoConstraint::P1080 => types::SessionCommandRequestVideoConstraintSubscribe::X1080p,
        client_simulator_config::VideoConstraint::P1440 => types::SessionCommandRequestVideoConstraintSubscribe::X1440p,
        client_simulator_config::VideoConstraint::P2160 => types::SessionCommandRequestVideoConstraintSubscribe::X2160p,
        client_simulator_config::VideoConstraint::P4320 => types::SessionCommandRequestVideoConstraintSubscribe::None,
    }
}

#[cfg(test)]
fn spawned_participants_for_test() -> &'static Mutex<Vec<String>> {
    static SPAWNED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    &SPAWNED
}

#[cfg(test)]
pub(crate) fn take_spawned_participants_for_test() -> Vec<String> {
    std::mem::take(&mut *spawned_participants_for_test().lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::{
        map_worker_browser_entry,
        CloudflareLaunchOptions,
        CloudflareSession,
    };
    use crate::participant::shared::{
        browser_log::BrowserLogSource,
        messages::ParticipantMessage,
        ParticipantDriverSession,
        ParticipantLaunchSpec,
        ParticipantSettings,
        ParticipantState,
        ResolvedFrontendKind,
    };
    use chrono::Utc;
    use client_simulator_config::{
        media::FakeMedia,
        CloudflareConfig,
        NoiseSuppression,
        OwnOrDefault,
        TransportMode,
        VideoConstraint,
        VideoMaxConcurrentTracks,
    };
    use serde_json::{
        json,
        Value,
    };
    use std::{
        collections::VecDeque,
        io::{
            Result as IoResult,
            Write,
        },
        sync::{
            Arc,
            Mutex,
        },
        time::Duration,
    };
    use tokio::{
        io::{
            AsyncReadExt as _,
            AsyncWriteExt as _,
        },
        net::TcpListener,
    };
    use url::Url;

    #[test]
    fn maps_worker_browser_entries() {
        let entry = cloudflare_worker_client::types::BrowserLogEntry {
            at: Utc::now(),
            level: "warning".to_owned(),
            message: "ICE connection state: disconnected".to_owned(),
            source: cloudflare_worker_client::types::BrowserLogEntrySource::Console,
        };

        let entry = map_worker_browser_entry("cf-owl-7", &entry);

        assert_eq!(entry.participant, "cf-owl-7");
        assert_eq!(entry.source, BrowserLogSource::Console);
        assert_eq!(entry.level, tracing::Level::WARN);
        assert_eq!(entry.text, "ICE connection state: disconnected");
    }

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        body: String,
    }

    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buffer: &[u8]) -> IoResult<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    struct CapturedLogs {
        output: Arc<Mutex<Vec<u8>>>,
        _guard: tracing::subscriber::DefaultGuard,
    }

    impl CapturedLogs {
        fn new() -> Self {
            let output = Arc::new(Mutex::new(Vec::new()));
            let writer_output = Arc::clone(&output);
            let subscriber = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_max_level(tracing::Level::TRACE)
                .with_writer(move || SharedBuffer(Arc::clone(&writer_output)))
                .finish();

            Self {
                output,
                _guard: tracing::subscriber::set_default(subscriber),
            }
        }

        fn output(&self) -> String {
            String::from_utf8(self.output.lock().unwrap().clone()).unwrap()
        }
    }

    #[tokio::test]
    async fn start_creates_worker_session_and_close_tears_it_down() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-123",
                    "state": {
                        "running": true,
                        "joined": true,
                        "muted": false,
                        "videoActivated": true,
                        "screenshareActivated": false,
                        "autoGainControl": true,
                        "noiseSuppression": "none",
                        "transportMode": "webrtc",
                        "videoConstraintPublishWebcam": "720p",
                        "videoConstraintSubscribe": "none",
                        "videoMaxConcurrentTracks": null,
                        "backgroundBlur": true
                    },
                    "log": [
                        {
                            "at": Utc::now().to_rfc3339(),
                            "step": "Joined the room"
                        }
                    ]
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-123",
                    "log": [
                        {
                            "at": Utc::now().to_rfc3339(),
                            "step": "Closed the browser"
                        }
                    ]
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperCore, &format!("{base_url}/room/demo")),
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: true,
                health_poll_interval_ms: 5_000,
            },
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert!(state.running);
        assert!(state.joined);
        assert_eq!(state.noise_suppression, NoiseSuppression::Disabled);
        assert_eq!(state.transport_mode, TransportMode::WebRTC);
        assert_eq!(state.video_constraint_publish_webcam, VideoConstraint::P720);
        assert_eq!(state.video_constraint_subscribe, VideoConstraint::None);
        assert_eq!(state.video_max_concurrent_tracks, VideoMaxConcurrentTracks::Unlimited);
        assert!(state.auto_gain_control);
        assert!(state.background_blur);

        session.close().await.unwrap();
        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);

        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/sessions");
        assert_eq!(
            serde_json::from_str::<Value>(&requests[0].body).unwrap(),
            json!({
                "debug": true,
                "browserLogs": false,
                "displayName": "cloudflare-sim",
                "frontendKind": "hyper-core",
                "navigationTimeoutMs": 30000.0,
                "roomUrl": format!("{base_url}/room/demo"),
                "selectorTimeoutMs": 10000.0,
                "sessionTimeoutMs": 120000.0,
                "settings": {
                    "audioEnabled": true,
                    "autoGainControl": true,
                    "blur": true,
                    "noiseSuppression": "none",
                    "screenshareEnabled": false,
                    "transport": "webrtc",
                    "videoConstraintPublishWebcam": "720p",
                    "videoConstraintSubscribe": "none",
                    "videoMaxConcurrentTracks": null,
                    "videoEnabled": true
                }
            })
        );

        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].path, "/sessions/cf-session-123/close");
    }

    /// The backend reports `none`, with a warning, when the worker reports an engine that hyper.video no longer offers.
    #[tokio::test]
    async fn start_reports_none_for_old_worker_engine() {
        let captured_logs = CapturedLogs::new();
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-old-engine",
                    "state": {
                        "running": true,
                        "joined": true,
                        "muted": false,
                        "videoActivated": true,
                        "screenshareActivated": false,
                        "autoGainControl": true,
                        "noiseSuppression": "ai-coustics-rook-s-48khz",
                        "transportMode": "webrtc",
                        "videoConstraintPublishWebcam": "720p",
                        "videoConstraintSubscribe": "none",
                        "videoMaxConcurrentTracks": null,
                        "backgroundBlur": true
                    },
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-old-engine",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, _requests, server) = spawn_http_server(responses).await;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo")),
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert!(state.auto_gain_control);
        assert_eq!(state.noise_suppression, NoiseSuppression::Disabled);
        assert!(captured_logs.output().contains(
            "reports noise suppression ai-coustics-rook-s-48khz, which hyper.video no longer offers; reporting none"
        ));

        session.close().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn start_forwards_worker_browser_logs() {
        let captured_logs = CapturedLogs::new();
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-browser-logs",
                    "state": worker_state_json(true, false, true, false, true, "none", "none", false),
                    "log": [],
                    "browserLog": [
                        {
                            "at": Utc::now().to_rfc3339(),
                            "level": "warning",
                            "source": "console",
                            "message": "ICE connection state: disconnected"
                        },
                        {
                            "at": Utc::now().to_rfc3339(),
                            "level": "error",
                            "source": "exception",
                            "message": "Uncaught TypeError: track is undefined"
                        }
                    ]
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-browser-logs",
                    "log": [],
                    "browserLog": []
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut options = launch_options(true, FakeMedia::None);
        options.browser_logs = true;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo")),
            options,
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();
        session.close().await.unwrap();
        server.abort();

        let logs = captured_logs.output();
        assert!(logs.contains("participant{name=cloudflare-sim}: browser:"));
        assert!(logs.contains("ICE connection state: disconnected"));
        assert!(logs.contains("source=console"));
        assert!(logs.contains("Uncaught TypeError: track is undefined"));
        assert!(logs.contains("source=exception"));
        assert!(!logs.contains("worker does not report browser logs"));

        let requests = requests.lock().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&requests[0].body).unwrap()["browserLogs"],
            true
        );
    }

    #[tokio::test]
    async fn start_reports_an_older_worker_without_browser_logs() {
        let captured_logs = CapturedLogs::new();
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-old-worker",
                    "state": worker_state_json(true, false, true, false, true, "none", "none", false),
                    "log": []
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-old-worker",
                    "log": []
                }),
            ),
        ]);
        let (base_url, _requests, server) = spawn_http_server(responses).await;
        let mut options = launch_options(true, FakeMedia::None);
        options.browser_logs = true;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo")),
            options,
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();
        session.close().await.unwrap();
        server.abort();

        assert!(captured_logs.output().contains("worker does not report browser logs"));
    }

    #[tokio::test]
    async fn commands_map_to_worker_requests_and_refresh_state_uses_cache() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(false, false, false, false, true, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, false, false, false, true, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, false, false, true, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, false, true, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, true, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "none", "1080p", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "none", "1080p", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(false, true, true, false, false, "none", "1080p", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo")),
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();

        let cases = vec![
            (
                ParticipantMessage::Join,
                json!({ "type": "join" }),
                expected_state(
                    true,
                    false,
                    false,
                    false,
                    true,
                    NoiseSuppression::Disabled,
                    VideoConstraint::None,
                    false,
                ),
            ),
            (
                ParticipantMessage::ToggleAudio,
                json!({ "type": "toggle-audio" }),
                expected_state(
                    true,
                    true,
                    false,
                    false,
                    true,
                    NoiseSuppression::Disabled,
                    VideoConstraint::None,
                    false,
                ),
            ),
            (
                ParticipantMessage::ToggleVideo,
                json!({ "type": "toggle-video" }),
                expected_state(
                    true,
                    true,
                    true,
                    false,
                    true,
                    NoiseSuppression::Disabled,
                    VideoConstraint::None,
                    false,
                ),
            ),
            (
                ParticipantMessage::ToggleScreenshare,
                json!({ "type": "toggle-screenshare" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    true,
                    NoiseSuppression::Disabled,
                    VideoConstraint::None,
                    false,
                ),
            ),
            (
                ParticipantMessage::ToggleAutoGainControl,
                json!({ "type": "toggle-auto-gain-control" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    false,
                    NoiseSuppression::Disabled,
                    VideoConstraint::None,
                    false,
                ),
            ),
            (
                ParticipantMessage::SetNoiseSuppression(NoiseSuppression::Disabled),
                json!({ "type": "set-noise-suppression", "noiseSuppression": "none" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    false,
                    NoiseSuppression::Disabled,
                    VideoConstraint::None,
                    false,
                ),
            ),
            (
                ParticipantMessage::SetVideoConstraintPublishWebcam(VideoConstraint::P1080),
                json!({ "type": "set-video-constraint-publish-webcam", "videoConstraintPublishWebcam": "1080p" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    false,
                    NoiseSuppression::Disabled,
                    VideoConstraint::P1080,
                    false,
                ),
            ),
            (
                ParticipantMessage::ToggleBackgroundBlur,
                json!({ "type": "toggle-background-blur" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    false,
                    NoiseSuppression::Disabled,
                    VideoConstraint::P1080,
                    true,
                ),
            ),
            (
                ParticipantMessage::Leave,
                json!({ "type": "leave" }),
                expected_state(
                    false,
                    true,
                    true,
                    false,
                    false,
                    NoiseSuppression::Disabled,
                    VideoConstraint::P1080,
                    true,
                ),
            ),
        ];

        for (index, (message, expected_body, expected_state)) in cases.into_iter().enumerate() {
            session.handle_command(message).await.unwrap();
            let request_count_before_refresh = requests.lock().unwrap().len();
            let state = session.refresh_state().await.unwrap();
            assert_eq!(
                requests.lock().unwrap().len(),
                request_count_before_refresh,
                "refresh_state unexpectedly triggered a network call for case {index}"
            );
            assert_state_matches(&state, &expected_state);
            let request = requests.lock().unwrap().get(index + 1).unwrap().clone();
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/sessions/cf-session-commands/commands");
            assert_eq!(serde_json::from_str::<Value>(&request.body).unwrap(), expected_body);
        }

        session.close().await.unwrap();
        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/sessions");
        assert_eq!(requests[10].method, "POST");
        assert_eq!(requests[10].path, "/sessions/cf-session-commands/close");
    }

    #[tokio::test]
    async fn start_normalizes_webtransport_to_webrtc_for_cloudflare() {
        let captured_logs = CapturedLogs::new();
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-webrtc",
                    "state": worker_state_json(false, false, false, false, true, "none", "720p", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-webrtc",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut spec = launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo"));
        spec.settings.transport = TransportMode::WebTransport;
        let mut session = CloudflareSession::new_for_test(
            spec,
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert_eq!(state.transport_mode, TransportMode::WebRTC);

        session.close().await.unwrap();
        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/sessions");
        assert_eq!(
            serde_json::from_str::<Value>(&requests[0].body).unwrap()["settings"]["transport"],
            json!("webrtc")
        );

        let logs = captured_logs.output();
        assert!(logs.contains("only supports WebRTC transport"));
        assert!(logs.contains("normalizing configured"));
    }

    #[tokio::test]
    async fn start_normalizes_noise_suppression_to_none_for_cloudflare() {
        let captured_logs = CapturedLogs::new();
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-noise-suppression",
                    "state": worker_state_json(false, false, false, false, true, "none", "720p", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-noise-suppression",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut spec = launch_spec(ResolvedFrontendKind::HyperCore, &format!("{base_url}/room/demo"));
        spec.settings.noise_suppression = NoiseSuppression::HyperVoice;
        let mut session = CloudflareSession::new_for_test(
            spec,
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert_eq!(state.noise_suppression, NoiseSuppression::Disabled);

        session.close().await.unwrap();
        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests[0].path, "/sessions");
        assert_eq!(
            serde_json::from_str::<Value>(&requests[0].body).unwrap()["settings"]["noiseSuppression"],
            json!("none")
        );

        let logs = captured_logs.output();
        assert!(logs.contains("only supports noise suppression none; normalizing configured hyper-voice to none"));
    }

    #[tokio::test]
    async fn set_noise_suppression_refuses_engines_for_cloudflare() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-set-noise-suppression",
                    "state": worker_state_json(true, false, false, false, true, "none", "720p", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-set-noise-suppression",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperCore, &format!("{base_url}/room/demo")),
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();

        let error = session
            .handle_command(ParticipantMessage::SetNoiseSuppression(NoiseSuppression::HyperVoice))
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Cloudflare backend only supports noise suppression none, not hyper-voice"
        );

        session.close().await.unwrap();
        server.abort();

        // Only the create and close requests reach the worker.
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/sessions");
        assert_eq!(requests[1].path, "/sessions/cf-session-set-noise-suppression/close");
    }

    #[tokio::test]
    async fn start_logs_ignored_headless_and_fake_media_settings_for_cloudflare() {
        let captured_logs = CapturedLogs::new();
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-limitations",
                    "state": worker_state_json(false, false, false, false, true, "none", "720p", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-limitations",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, _requests, server) = spawn_http_server(responses).await;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo")),
            launch_options(
                false,
                FakeMedia::FileOrUrl("https://example.com/fake-media.mp4".to_owned()),
            ),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 60_000,
            },
        );

        session.start().await.unwrap();
        session.close().await.unwrap();
        server.abort();

        let logs = captured_logs.output();
        assert!(logs.contains("ignores headless=false"));
        assert!(logs.contains("ignores local fake media source"));
        assert!(logs.contains("https://example.com/fake-media.mp4"));
    }

    #[tokio::test]
    async fn termination_poller_reports_worker_state_failures() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-terminated",
                    "state": worker_state_json(true, false, false, false, true, "none", "none", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                500,
                json!({
                    "ok": false,
                    "sessionId": "cf-session-terminated",
                    "error": "Browser session missing",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, requests, server) = spawn_http_server(responses).await;
        let mut session = CloudflareSession::new_for_test(
            launch_spec(ResolvedFrontendKind::HyperLite, &format!("{base_url}/room/demo")),
            launch_options(false, FakeMedia::None),
            CloudflareConfig {
                base_url: Url::parse(&base_url).unwrap(),
                request_timeout_seconds: 5,
                session_timeout_ms: 120_000,
                navigation_timeout_ms: 30_000,
                selector_timeout_ms: 10_000,
                debug: false,
                health_poll_interval_ms: 5,
            },
        );

        session.start().await.unwrap();

        let termination = tokio::time::timeout(Duration::from_secs(1), session.wait_for_termination())
            .await
            .expect("timed out waiting for Cloudflare termination");

        assert_eq!(termination.level, "warn");
        assert!(termination.message.contains("cf-session-terminated"));
        assert!(termination.message.contains("Browser session missing"));

        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].path, "/sessions/cf-session-terminated/keep-alive");
    }

    fn launch_options(headless: bool, fake_media: FakeMedia) -> CloudflareLaunchOptions {
        CloudflareLaunchOptions {
            headless,
            fake_media,
            browser_logs: false,
        }
    }

    fn launch_spec(frontend_kind: ResolvedFrontendKind, room_url: &str) -> ParticipantLaunchSpec {
        ParticipantLaunchSpec {
            username: "cloudflare-sim".to_owned(),
            session_url: Url::parse(room_url).unwrap(),
            frontend_kind,
            settings: ParticipantSettings {
                audio_enabled: true,
                video_enabled: true,
                screenshare_enabled: false,
                auto_gain_control: OwnOrDefault::Own(true),
                noise_suppression: NoiseSuppression::Disabled,
                transport: TransportMode::WebRTC,
                video_constraint_publish_webcam: OwnOrDefault::Own(VideoConstraint::P720),
                video_constraint_subscribe: OwnOrDefault::Default,
                video_max_concurrent_tracks: OwnOrDefault::Default,
                blur: OwnOrDefault::Own(true),
            },
        }
    }

    fn worker_state_json(
        joined: bool,
        muted: bool,
        video_activated: bool,
        screenshare_activated: bool,
        auto_gain_control: bool,
        noise_suppression: &str,
        video_constraint_publish_webcam: &str,
        background_blur: bool,
    ) -> Value {
        json!({
            "running": true,
            "joined": joined,
            "muted": muted,
            "videoActivated": video_activated,
            "screenshareActivated": screenshare_activated,
            "autoGainControl": auto_gain_control,
            "noiseSuppression": noise_suppression,
            "transportMode": "webrtc",
            "videoConstraintPublishWebcam": video_constraint_publish_webcam,
            "videoConstraintSubscribe": "none",
            "videoMaxConcurrentTracks": null,
            "backgroundBlur": background_blur,
        })
    }

    fn expected_state(
        joined: bool,
        muted: bool,
        video_activated: bool,
        screenshare_activated: bool,
        auto_gain_control: bool,
        noise_suppression: NoiseSuppression,
        video_constraint_publish_webcam: VideoConstraint,
        background_blur: bool,
    ) -> ParticipantState {
        ParticipantState {
            username: String::new(),
            running: true,
            warning: None,
            joined,
            muted,
            video_activated,
            auto_gain_control,
            noise_suppression,
            transport_mode: TransportMode::WebRTC,
            video_constraint_publish_webcam,
            video_constraint_subscribe: VideoConstraint::None,
            video_max_concurrent_tracks: VideoMaxConcurrentTracks::Unlimited,
            background_blur,
            screenshare_activated,
        }
    }

    fn assert_state_matches(actual: &ParticipantState, expected: &ParticipantState) {
        assert_eq!(actual.username, expected.username);
        assert_eq!(actual.running, expected.running);
        assert_eq!(actual.joined, expected.joined);
        assert_eq!(actual.muted, expected.muted);
        assert_eq!(actual.video_activated, expected.video_activated);
        assert_eq!(actual.auto_gain_control, expected.auto_gain_control);
        assert_eq!(actual.noise_suppression, expected.noise_suppression);
        assert_eq!(actual.transport_mode, expected.transport_mode);
        assert_eq!(
            actual.video_constraint_publish_webcam,
            expected.video_constraint_publish_webcam
        );
        assert_eq!(actual.video_constraint_subscribe, expected.video_constraint_subscribe);
        assert_eq!(actual.video_max_concurrent_tracks, expected.video_max_concurrent_tracks);
        assert_eq!(actual.background_blur, expected.background_blur);
        assert_eq!(actual.screenshare_activated, expected.screenshare_activated);
    }

    async fn spawn_http_server(
        responses: VecDeque<MockResponse>,
    ) -> (String, Arc<Mutex<Vec<CapturedRequest>>>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_task = Arc::clone(&requests);

        let task = tokio::spawn(async move {
            let mut responses = responses;

            while let Some(response) = responses.pop_front() {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                requests_for_task.lock().unwrap().push(request);
                let reply = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                    response.status,
                    status_text(response.status),
                    response.body.len(),
                    response.headers,
                    response.body,
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
            }
        });

        (base_url, requests, task)
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) -> CapturedRequest {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end;

        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0, "unexpected EOF while reading request headers");
            buffer.extend_from_slice(&chunk[..read]);

            if let Some(end) = find_header_end(&buffer) {
                header_end = end;
                break;
            }
        }

        let headers_bytes = &buffer[..header_end];
        let headers_text = String::from_utf8(headers_bytes.to_vec()).unwrap();
        let mut lines = headers_text.split("\r\n");
        let request_line = lines.next().unwrap();
        let mut request_line = request_line.split_whitespace();
        let method = request_line.next().unwrap().to_owned();
        let path = request_line.next().unwrap().to_owned();

        let mut content_length = 0_usize;
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, value) = line.split_once(':').unwrap();
            let value = value.trim().to_owned();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap();
            }
        }

        let body_start = header_end + 4;
        let mut body = buffer[body_start..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0, "unexpected EOF while reading request body");
            body.extend_from_slice(&chunk[..read]);
        }

        CapturedRequest {
            method,
            path,
            body: String::from_utf8(body).unwrap(),
        }
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn status_text(status: u16) -> &'static str {
        match status {
            200 => "OK",
            500 => "Internal Server Error",
            _ => "OK",
        }
    }

    struct MockResponse {
        status: u16,
        headers: String,
        body: String,
    }

    impl MockResponse {
        fn json(status: u16, body: Value) -> Self {
            Self {
                status,
                headers: String::new(),
                body: serde_json::to_string(&body).unwrap(),
            }
        }
    }
}
