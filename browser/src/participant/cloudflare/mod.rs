use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookieManger,
    },
    participant::shared::{
        messages::{
            ParticipantLogMessage,
            ParticipantMessage,
        },
        DriverTermination,
        ParticipantDriverSession,
        ParticipantLaunchSpec,
        ParticipantState,
        ResolvedFrontendKind,
    },
};
use client_simulator_config::{
    media::FakeMedia,
    CloudflareConfig,
    TransportMode,
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
        mpsc::UnboundedSender,
        oneshot,
        watch,
    },
    task::JoinHandle,
    time::MissedTickBehavior,
};

enum CloudflareAuth {
    HyperCore {
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    },
    HyperLite,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CloudflareLaunchOptions {
    headless: bool,
    fake_media: FakeMedia,
}

impl From<&client_simulator_config::Config> for CloudflareLaunchOptions {
    fn from(config: &client_simulator_config::Config) -> Self {
        Self {
            headless: config.headless,
            fake_media: config.fake_media(),
        }
    }
}

pub(super) struct CloudflareSession {
    launch_spec: ParticipantLaunchSpec,
    launch_options: CloudflareLaunchOptions,
    cloudflare_config: CloudflareConfig,
    sender: UnboundedSender<ParticipantLogMessage>,
    auth: CloudflareAuth,
    session_id: Option<String>,
    cached_state: Arc<Mutex<ParticipantState>>,
    termination_tx: watch::Sender<Option<DriverTermination>>,
    termination_rx: watch::Receiver<Option<DriverTermination>>,
    poller_shutdown_tx: Option<oneshot::Sender<()>>,
    poller_task: Option<JoinHandle<()>>,
}

fn emit_log_message(
    sender: &UnboundedSender<ParticipantLogMessage>,
    participant_name: &str,
    level: &str,
    message: impl ToString,
) {
    let log_message = ParticipantLogMessage::new(level, participant_name, message);
    log_message.write();
    if let Err(err) = sender.send(log_message) {
        trace!(
            participant = %participant_name,
            "Failed to send cloudflare driver log message: {err}"
        );
    }
}

fn forward_worker_entries(
    sender: &UnboundedSender<ParticipantLogMessage>,
    participant_name: &str,
    entries: &[types::AutomationLogEntry],
) {
    for entry in entries {
        emit_log_message(
            sender,
            participant_name,
            "debug",
            format!("worker {} {}", entry.at.to_rfc3339(), entry.step),
        );
    }
}

fn store_cached_state(cached_state: &Arc<Mutex<ParticipantState>>, state: &types::ParticipantState) {
    *cached_state.lock().unwrap() = map_state(state);
}

impl CloudflareSession {
    pub(super) fn new(
        launch_spec: ParticipantLaunchSpec,
        launch_options: CloudflareLaunchOptions,
        cloudflare_config: CloudflareConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Self {
        Self::build(
            launch_spec,
            launch_options,
            cloudflare_config,
            sender,
            cookie,
            cookie_manager,
            true,
        )
    }

    fn build(
        launch_spec: ParticipantLaunchSpec,
        launch_options: CloudflareLaunchOptions,
        cloudflare_config: CloudflareConfig,
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
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

        let auth = match launch_spec.frontend_kind {
            ResolvedFrontendKind::HyperCore => CloudflareAuth::HyperCore { cookie, cookie_manager },
            ResolvedFrontendKind::HyperLite => CloudflareAuth::HyperLite,
        };
        let (termination_tx, termination_rx) = watch::channel(None);

        Self {
            cached_state: Arc::new(Mutex::new(ParticipantState {
                username: launch_spec.username.clone(),
                ..Default::default()
            })),
            launch_spec,
            launch_options,
            cloudflare_config,
            sender,
            auth,
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
        sender: UnboundedSender<ParticipantLogMessage>,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Self {
        Self::build(
            launch_spec,
            launch_options,
            cloudflare_config,
            sender,
            cookie,
            cookie_manager,
            false,
        )
    }

    fn log_message(&self, level: &str, message: impl ToString) {
        emit_log_message(&self.sender, &self.launch_spec.username, level, message);
    }

    fn worker_client(&self) -> Result<CloudflareWorkerClient> {
        CloudflareWorkerClient::new(
            self.cloudflare_config.base_url.as_ref(),
            Duration::from_secs(self.cloudflare_config.request_timeout_seconds),
        )
        .wrap_err("Failed to construct Cloudflare worker client")
    }

    fn log_worker_entries(&self, entries: &[types::AutomationLogEntry]) {
        forward_worker_entries(&self.sender, &self.launch_spec.username, entries);
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

        settings
    }

    async fn ensure_hyper_session_cookie(&mut self) -> Result<Option<String>> {
        match &mut self.auth {
            CloudflareAuth::HyperCore { cookie, cookie_manager } => {
                if cookie.is_none() {
                    *cookie = Some(
                        cookie_manager
                            .give_or_fetch_cookie(self.launch_spec.base_url(), &self.launch_spec.username)
                            .await?,
                    );
                }

                Ok(cookie.as_ref().map(|cookie| cookie.raw_value().to_owned()))
            }
            CloudflareAuth::HyperLite => Ok(None),
        }
    }

    async fn build_create_request(&mut self) -> Result<types::SessionCreateRequest> {
        self.log_backend_limitations();
        let normalized_settings = self.normalized_settings();
        let hyper_session_cookie = self
            .ensure_hyper_session_cookie()
            .await?
            .map(types::SessionCreateRequestHyperSessionCookie::try_from)
            .transpose()
            .map_err(|error| eyre!("Failed to encode Hyper Core session cookie for the worker: {error}"))?;

        Ok(types::SessionCreateRequest {
            debug: Some(self.cloudflare_config.debug),
            display_name: types::SessionCreateRequestDisplayName::try_from(self.launch_spec.username.clone())
                .map_err(|error| eyre!("Invalid Cloudflare display name: {error}"))?,
            frontend_kind: map_frontend_kind(self.launch_spec.frontend_kind),
            hyper_session_cookie,
            navigation_timeout_ms: Some(self.cloudflare_config.navigation_timeout_ms as f64),
            room_url: self.launch_spec.session_url.to_string(),
            selector_timeout_ms: Some(self.cloudflare_config.selector_timeout_ms as f64),
            session_timeout_ms: Some(self.cloudflare_config.session_timeout_ms as f64),
            settings: map_settings(&normalized_settings),
        })
    }

    fn command_request(message: ParticipantMessage) -> types::SessionCommandRequest {
        match message {
            ParticipantMessage::Join => types::SessionCommandRequest::Join,
            ParticipantMessage::Leave => types::SessionCommandRequest::Leave,
            ParticipantMessage::Close => types::SessionCommandRequest::Leave,
            ParticipantMessage::ToggleAudio => types::SessionCommandRequest::ToggleAudio,
            ParticipantMessage::ToggleVideo => types::SessionCommandRequest::ToggleVideo,
            ParticipantMessage::ToggleScreenshare => types::SessionCommandRequest::ToggleScreenshare,
            ParticipantMessage::ToggleAutoGainControl => types::SessionCommandRequest::ToggleAutoGainControl,
            ParticipantMessage::SetNoiseSuppression(value) => types::SessionCommandRequest::SetNoiseSuppression {
                noise_suppression: map_command_noise_suppression(value),
            },
            ParticipantMessage::SetWebcamResolutions(value) => types::SessionCommandRequest::SetWebcamResolution {
                webcam_resolution: map_command_webcam_resolution(value),
            },
            ParticipantMessage::ToggleBackgroundBlur => types::SessionCommandRequest::ToggleBackgroundBlur,
        }
    }

    fn cached_state(&self) -> ParticipantState {
        self.cached_state.lock().unwrap().clone()
    }

    fn update_cached_state(&self, state: &types::ParticipantState) {
        store_cached_state(&self.cached_state, state);
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
        let sender = self.sender.clone();
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
                                forward_worker_entries(&sender, &participant_name, &response.log);

                                store_cached_state(&cached_state, &response.state);

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
        let request = Self::command_request(message);
        let response = self.worker_client()?.command_session(&session_id, &request).await?;
        self.log_worker_entries(&response.log);
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

fn map_settings(settings: &crate::participant::shared::ParticipantSettings) -> types::ParticipantSettings {
    types::ParticipantSettings {
        audio_enabled: settings.audio_enabled,
        auto_gain_control: settings.auto_gain_control,
        blur: settings.blur,
        noise_suppression: match settings.noise_suppression {
            client_simulator_config::NoiseSuppression::Disabled => types::ParticipantSettingsNoiseSuppression::None,
            client_simulator_config::NoiseSuppression::Deepfilternet => {
                types::ParticipantSettingsNoiseSuppression::Deepfilternet
            }
            client_simulator_config::NoiseSuppression::RNNoise => types::ParticipantSettingsNoiseSuppression::Rnnoise,
            client_simulator_config::NoiseSuppression::IRISCarthy => {
                types::ParticipantSettingsNoiseSuppression::IrisCarthy
            }
            client_simulator_config::NoiseSuppression::KrispHigh => {
                types::ParticipantSettingsNoiseSuppression::KrispHigh
            }
            client_simulator_config::NoiseSuppression::KrispMedium => {
                types::ParticipantSettingsNoiseSuppression::KrispMedium
            }
            client_simulator_config::NoiseSuppression::KrispLow => types::ParticipantSettingsNoiseSuppression::KrispLow,
            client_simulator_config::NoiseSuppression::KrispHighWithBVC => {
                types::ParticipantSettingsNoiseSuppression::KrispHighWithBvc
            }
            client_simulator_config::NoiseSuppression::KrispMediumWithBVC => {
                types::ParticipantSettingsNoiseSuppression::KrispMediumWithBvc
            }
            client_simulator_config::NoiseSuppression::AiCousticsSparrowXxs => {
                types::ParticipantSettingsNoiseSuppression::AiCousticsSparrowXxs
            }
            client_simulator_config::NoiseSuppression::AiCousticsSparrowXs => {
                types::ParticipantSettingsNoiseSuppression::AiCousticsSparrowXs
            }
            client_simulator_config::NoiseSuppression::AiCousticsSparrowS => {
                types::ParticipantSettingsNoiseSuppression::AiCousticsSparrowS
            }
            client_simulator_config::NoiseSuppression::AiCousticsSparrowL => {
                types::ParticipantSettingsNoiseSuppression::AiCousticsSparrowL
            }
        },
        resolution: match settings.resolution {
            client_simulator_config::WebcamResolution::Auto => types::ParticipantSettingsResolution::Auto,
            client_simulator_config::WebcamResolution::P144 => types::ParticipantSettingsResolution::P144,
            client_simulator_config::WebcamResolution::P240 => types::ParticipantSettingsResolution::P240,
            client_simulator_config::WebcamResolution::P360 => types::ParticipantSettingsResolution::P360,
            client_simulator_config::WebcamResolution::P480 => types::ParticipantSettingsResolution::P480,
            client_simulator_config::WebcamResolution::P720 => types::ParticipantSettingsResolution::P720,
            client_simulator_config::WebcamResolution::P1080 => types::ParticipantSettingsResolution::P1080,
            client_simulator_config::WebcamResolution::P1440 => types::ParticipantSettingsResolution::P1440,
            client_simulator_config::WebcamResolution::P2160 => types::ParticipantSettingsResolution::P2160,
            client_simulator_config::WebcamResolution::P4320 => types::ParticipantSettingsResolution::P4320,
        },
        screenshare_enabled: settings.screenshare_enabled,
        transport: match settings.transport {
            client_simulator_config::TransportMode::WebRTC => types::ParticipantSettingsTransport::Webrtc,
            client_simulator_config::TransportMode::WebTransport => types::ParticipantSettingsTransport::Webtransport,
        },
        video_enabled: settings.video_enabled,
    }
}

fn map_state(state: &types::ParticipantState) -> ParticipantState {
    ParticipantState {
        username: String::new(),
        running: state.running,
        joined: state.joined,
        muted: state.muted,
        video_activated: state.video_activated,
        auto_gain_control: state.auto_gain_control,
        noise_suppression: match state.noise_suppression {
            types::ParticipantStateNoiseSuppression::None => client_simulator_config::NoiseSuppression::Disabled,
            types::ParticipantStateNoiseSuppression::Deepfilternet => {
                client_simulator_config::NoiseSuppression::Deepfilternet
            }
            types::ParticipantStateNoiseSuppression::Rnnoise => client_simulator_config::NoiseSuppression::RNNoise,
            types::ParticipantStateNoiseSuppression::IrisCarthy => {
                client_simulator_config::NoiseSuppression::IRISCarthy
            }
            types::ParticipantStateNoiseSuppression::KrispHigh => client_simulator_config::NoiseSuppression::KrispHigh,
            types::ParticipantStateNoiseSuppression::KrispMedium => {
                client_simulator_config::NoiseSuppression::KrispMedium
            }
            types::ParticipantStateNoiseSuppression::KrispLow => client_simulator_config::NoiseSuppression::KrispLow,
            types::ParticipantStateNoiseSuppression::KrispHighWithBvc => {
                client_simulator_config::NoiseSuppression::KrispHighWithBVC
            }
            types::ParticipantStateNoiseSuppression::KrispMediumWithBvc => {
                client_simulator_config::NoiseSuppression::KrispMediumWithBVC
            }
            types::ParticipantStateNoiseSuppression::AiCousticsSparrowXxs => {
                client_simulator_config::NoiseSuppression::AiCousticsSparrowXxs
            }
            types::ParticipantStateNoiseSuppression::AiCousticsSparrowXs => {
                client_simulator_config::NoiseSuppression::AiCousticsSparrowXs
            }
            types::ParticipantStateNoiseSuppression::AiCousticsSparrowS => {
                client_simulator_config::NoiseSuppression::AiCousticsSparrowS
            }
            types::ParticipantStateNoiseSuppression::AiCousticsSparrowL => {
                client_simulator_config::NoiseSuppression::AiCousticsSparrowL
            }
        },
        transport_mode: match state.transport_mode {
            types::ParticipantStateTransportMode::Webrtc => client_simulator_config::TransportMode::WebRTC,
            types::ParticipantStateTransportMode::Webtransport => client_simulator_config::TransportMode::WebTransport,
        },
        webcam_resolution: match state.webcam_resolution {
            types::ParticipantStateWebcamResolution::Auto => client_simulator_config::WebcamResolution::Auto,
            types::ParticipantStateWebcamResolution::P144 => client_simulator_config::WebcamResolution::P144,
            types::ParticipantStateWebcamResolution::P240 => client_simulator_config::WebcamResolution::P240,
            types::ParticipantStateWebcamResolution::P360 => client_simulator_config::WebcamResolution::P360,
            types::ParticipantStateWebcamResolution::P480 => client_simulator_config::WebcamResolution::P480,
            types::ParticipantStateWebcamResolution::P720 => client_simulator_config::WebcamResolution::P720,
            types::ParticipantStateWebcamResolution::P1080 => client_simulator_config::WebcamResolution::P1080,
            types::ParticipantStateWebcamResolution::P1440 => client_simulator_config::WebcamResolution::P1440,
            types::ParticipantStateWebcamResolution::P2160 => client_simulator_config::WebcamResolution::P2160,
            types::ParticipantStateWebcamResolution::P4320 => client_simulator_config::WebcamResolution::P4320,
        },
        background_blur: state.background_blur,
        screenshare_activated: state.screenshare_activated,
    }
}

fn map_command_noise_suppression(
    noise_suppression: client_simulator_config::NoiseSuppression,
) -> types::SessionCommandRequestNoiseSuppression {
    match noise_suppression {
        client_simulator_config::NoiseSuppression::Disabled => types::SessionCommandRequestNoiseSuppression::None,
        client_simulator_config::NoiseSuppression::Deepfilternet => {
            types::SessionCommandRequestNoiseSuppression::Deepfilternet
        }
        client_simulator_config::NoiseSuppression::RNNoise => types::SessionCommandRequestNoiseSuppression::Rnnoise,
        client_simulator_config::NoiseSuppression::IRISCarthy => {
            types::SessionCommandRequestNoiseSuppression::IrisCarthy
        }
        client_simulator_config::NoiseSuppression::KrispHigh => types::SessionCommandRequestNoiseSuppression::KrispHigh,
        client_simulator_config::NoiseSuppression::KrispMedium => {
            types::SessionCommandRequestNoiseSuppression::KrispMedium
        }
        client_simulator_config::NoiseSuppression::KrispLow => types::SessionCommandRequestNoiseSuppression::KrispLow,
        client_simulator_config::NoiseSuppression::KrispHighWithBVC => {
            types::SessionCommandRequestNoiseSuppression::KrispHighWithBvc
        }
        client_simulator_config::NoiseSuppression::KrispMediumWithBVC => {
            types::SessionCommandRequestNoiseSuppression::KrispMediumWithBvc
        }
        client_simulator_config::NoiseSuppression::AiCousticsSparrowXxs => {
            types::SessionCommandRequestNoiseSuppression::AiCousticsSparrowXxs
        }
        client_simulator_config::NoiseSuppression::AiCousticsSparrowXs => {
            types::SessionCommandRequestNoiseSuppression::AiCousticsSparrowXs
        }
        client_simulator_config::NoiseSuppression::AiCousticsSparrowS => {
            types::SessionCommandRequestNoiseSuppression::AiCousticsSparrowS
        }
        client_simulator_config::NoiseSuppression::AiCousticsSparrowL => {
            types::SessionCommandRequestNoiseSuppression::AiCousticsSparrowL
        }
    }
}

fn map_command_webcam_resolution(
    webcam_resolution: client_simulator_config::WebcamResolution,
) -> types::SessionCommandRequestWebcamResolution {
    match webcam_resolution {
        client_simulator_config::WebcamResolution::Auto => types::SessionCommandRequestWebcamResolution::Auto,
        client_simulator_config::WebcamResolution::P144 => types::SessionCommandRequestWebcamResolution::P144,
        client_simulator_config::WebcamResolution::P240 => types::SessionCommandRequestWebcamResolution::P240,
        client_simulator_config::WebcamResolution::P360 => types::SessionCommandRequestWebcamResolution::P360,
        client_simulator_config::WebcamResolution::P480 => types::SessionCommandRequestWebcamResolution::P480,
        client_simulator_config::WebcamResolution::P720 => types::SessionCommandRequestWebcamResolution::P720,
        client_simulator_config::WebcamResolution::P1080 => types::SessionCommandRequestWebcamResolution::P1080,
        client_simulator_config::WebcamResolution::P1440 => types::SessionCommandRequestWebcamResolution::P1440,
        client_simulator_config::WebcamResolution::P2160 => types::SessionCommandRequestWebcamResolution::P2160,
        client_simulator_config::WebcamResolution::P4320 => types::SessionCommandRequestWebcamResolution::P4320,
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
        CloudflareLaunchOptions,
        CloudflareSession,
    };
    use crate::{
        auth::HyperSessionCookieManger,
        participant::shared::{
            messages::ParticipantMessage,
            ParticipantDriverSession,
            ParticipantLaunchSpec,
            ParticipantSettings,
            ParticipantState,
            ResolvedFrontendKind,
        },
    };
    use chrono::Utc;
    use client_simulator_config::{
        media::FakeMedia,
        CloudflareConfig,
        NoiseSuppression,
        TransportMode,
        WebcamResolution,
    };
    use serde_json::{
        json,
        Value,
    };
    use std::{
        collections::VecDeque,
        fs,
        path::PathBuf,
        sync::{
            Arc,
            Mutex,
        },
        time::{
            Duration,
            SystemTime,
            UNIX_EPOCH,
        },
    };
    use tokio::{
        io::{
            AsyncReadExt as _,
            AsyncWriteExt as _,
        },
        net::TcpListener,
        sync::mpsc::unbounded_channel,
    };
    use url::Url;

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    #[tokio::test]
    async fn start_fetches_cookie_creates_worker_session_and_close_tears_it_down() {
        let responses = VecDeque::from(vec![
            MockResponse::new(
                200,
                "Set-Cookie: hyper_session=fetched-cookie; Path=/; HttpOnly\r\n",
                "",
            ),
            MockResponse::json(200, json!({ "ok": true })),
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
                        "noiseSuppression": "rnnoise",
                        "transportMode": "webrtc",
                        "webcamResolution": "p720",
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
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, _log_receiver) = unbounded_channel();
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
            log_sender,
            None,
            cookie_manager,
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert!(state.running);
        assert!(state.joined);
        assert_eq!(state.noise_suppression, NoiseSuppression::RNNoise);
        assert_eq!(state.transport_mode, TransportMode::WebRTC);
        assert_eq!(state.webcam_resolution, WebcamResolution::P720);
        assert!(state.auto_gain_control);
        assert!(state.background_blur);

        session.close().await.unwrap();
        server.abort();

        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);

        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/api/v1/auth/guest?username=guest");

        assert_eq!(requests[1].method, "PUT");
        assert_eq!(requests[1].path, "/api/v1/auth/me/name");
        assert_eq!(
            header_value(&requests[1], "cookie").as_deref(),
            Some("hyper_session=fetched-cookie")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&requests[1].body).unwrap(),
            json!({ "name": "cloudflare-sim" })
        );

        assert_eq!(requests[2].method, "POST");
        assert_eq!(requests[2].path, "/sessions");
        assert_eq!(
            serde_json::from_str::<Value>(&requests[2].body).unwrap(),
            json!({
                "debug": true,
                "displayName": "cloudflare-sim",
                "frontendKind": "hyper-core",
                "hyperSessionCookie": "fetched-cookie",
                "navigationTimeoutMs": 30000.0,
                "roomUrl": format!("{base_url}/room/demo"),
                "selectorTimeoutMs": 10000.0,
                "sessionTimeoutMs": 120000.0,
                "settings": {
                    "audioEnabled": true,
                    "autoGainControl": true,
                    "blur": true,
                    "noiseSuppression": "rnnoise",
                    "resolution": "p720",
                    "screenshareEnabled": false,
                    "transport": "webrtc",
                    "videoEnabled": true
                }
            })
        );

        assert_eq!(requests[3].method, "POST");
        assert_eq!(requests[3].path, "/sessions/cf-session-123/close");
    }

    #[tokio::test]
    async fn start_accepts_ai_coustics_noise_suppression_from_worker_state() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-ai-coustics",
                    "state": {
                        "running": true,
                        "joined": true,
                        "muted": false,
                        "videoActivated": true,
                        "screenshareActivated": false,
                        "autoGainControl": true,
                        "noiseSuppression": "ai-coustics-sparrow-s",
                        "transportMode": "webrtc",
                        "webcamResolution": "p720",
                        "backgroundBlur": true
                    },
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-ai-coustics",
                    "log": [],
                }),
            ),
        ]);
        let (base_url, _requests, server) = spawn_http_server(responses).await;
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, _log_receiver) = unbounded_channel();
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
            log_sender,
            None,
            cookie_manager,
        );

        session.start().await.unwrap();

        let state = session.refresh_state().await.unwrap();
        assert!(state.auto_gain_control);
        assert_eq!(state.noise_suppression, NoiseSuppression::AiCousticsSparrowS);

        session.close().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn commands_map_to_worker_requests_and_refresh_state_uses_cache() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(false, false, false, false, true, "none", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, false, false, false, true, "none", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, false, false, true, "none", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, false, true, "none", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, true, "none", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "none", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "deepfilternet", "auto", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "deepfilternet", "p1080", false),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(true, true, true, true, false, "deepfilternet", "p1080", true),
                    "log": [],
                }),
            ),
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-commands",
                    "state": worker_state_json(false, true, true, false, false, "deepfilternet", "p1080", true),
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
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, _log_receiver) = unbounded_channel();
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
            log_sender,
            None,
            cookie_manager,
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
                    WebcamResolution::Auto,
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
                    WebcamResolution::Auto,
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
                    WebcamResolution::Auto,
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
                    WebcamResolution::Auto,
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
                    WebcamResolution::Auto,
                    false,
                ),
            ),
            (
                ParticipantMessage::SetNoiseSuppression(NoiseSuppression::Deepfilternet),
                json!({ "type": "set-noise-suppression", "noiseSuppression": "deepfilternet" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    false,
                    NoiseSuppression::Deepfilternet,
                    WebcamResolution::Auto,
                    false,
                ),
            ),
            (
                ParticipantMessage::SetWebcamResolutions(WebcamResolution::P1080),
                json!({ "type": "set-webcam-resolution", "webcamResolution": "p1080" }),
                expected_state(
                    true,
                    true,
                    true,
                    true,
                    false,
                    NoiseSuppression::Deepfilternet,
                    WebcamResolution::P1080,
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
                    NoiseSuppression::Deepfilternet,
                    WebcamResolution::P1080,
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
                    NoiseSuppression::Deepfilternet,
                    WebcamResolution::P1080,
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
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-webrtc",
                    "state": worker_state_json(false, false, false, false, true, "rnnoise", "p720", true),
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
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, mut log_receiver) = unbounded_channel();
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
            log_sender,
            None,
            cookie_manager,
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

        let logs = drain_log_messages(&mut log_receiver);
        assert!(logs.iter().any(|message| {
            message.contains("only supports WebRTC transport") && message.contains("normalizing configured")
        }));
    }

    #[tokio::test]
    async fn start_logs_ignored_headless_and_fake_media_settings_for_cloudflare() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-limitations",
                    "state": worker_state_json(false, false, false, false, true, "rnnoise", "p720", true),
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
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, mut log_receiver) = unbounded_channel();
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
            log_sender,
            None,
            cookie_manager,
        );

        session.start().await.unwrap();
        session.close().await.unwrap();
        server.abort();

        let logs = drain_log_messages(&mut log_receiver);
        assert!(logs.iter().any(|message| message.contains("ignores headless=false")));
        assert!(logs.iter().any(|message| {
            message.contains("ignores local fake media source")
                && message.contains("https://example.com/fake-media.mp4")
        }));
    }

    #[tokio::test]
    async fn termination_poller_reports_worker_state_failures() {
        let responses = VecDeque::from(vec![
            MockResponse::json(
                200,
                json!({
                    "ok": true,
                    "sessionId": "cf-session-terminated",
                    "state": worker_state_json(true, false, false, false, true, "none", "auto", false),
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
        let cookie_manager = HyperSessionCookieManger::new(unique_temp_dir().join("cookies.json"));
        let (log_sender, _log_receiver) = unbounded_channel();
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
            log_sender,
            None,
            cookie_manager,
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
        CloudflareLaunchOptions { headless, fake_media }
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
                auto_gain_control: true,
                noise_suppression: NoiseSuppression::RNNoise,
                transport: TransportMode::WebRTC,
                resolution: WebcamResolution::P720,
                blur: true,
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
        webcam_resolution: &str,
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
            "webcamResolution": webcam_resolution,
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
        webcam_resolution: WebcamResolution,
        background_blur: bool,
    ) -> ParticipantState {
        ParticipantState {
            username: String::new(),
            running: true,
            joined,
            muted,
            video_activated,
            auto_gain_control,
            noise_suppression,
            transport_mode: TransportMode::WebRTC,
            webcam_resolution,
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
        assert_eq!(actual.webcam_resolution, expected.webcam_resolution);
        assert_eq!(actual.background_blur, expected.background_blur);
        assert_eq!(actual.screenshare_activated, expected.screenshare_activated);
    }

    fn drain_log_messages(
        log_receiver: &mut tokio::sync::mpsc::UnboundedReceiver<
            crate::participant::shared::messages::ParticipantLogMessage,
        >,
    ) -> Vec<String> {
        let mut messages = Vec::new();
        while let Ok(message) = log_receiver.try_recv() {
            messages.push(message.message);
        }
        messages
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

        let mut headers = Vec::new();
        let mut content_length = 0_usize;
        for line in lines.filter(|line| !line.is_empty()) {
            let (name, value) = line.split_once(':').unwrap();
            let value = value.trim().to_owned();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap();
            }
            headers.push((name.to_ascii_lowercase(), value));
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
            headers,
            body: String::from_utf8(body).unwrap(),
        }
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn header_value(request: &CapturedRequest, name: &str) -> Option<String> {
        request
            .headers
            .iter()
            .find(|(header_name, _)| header_name == &name.to_ascii_lowercase())
            .map(|(_, value)| value.clone())
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("hyper-browser-simulator-cloudflare-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        dir
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
        fn new(status: u16, headers: &str, body: &str) -> Self {
            Self {
                status,
                headers: headers.to_owned(),
                body: body.to_owned(),
            }
        }

        fn json(status: u16, body: Value) -> Self {
            Self {
                status,
                headers: String::new(),
                body: serde_json::to_string(&body).unwrap(),
            }
        }
    }
}
