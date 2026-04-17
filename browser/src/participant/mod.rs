use super::auth::{
    BorrowedCookie,
    HyperSessionCookieManger,
};
use crate::participant::{
    local::session::LocalChromiumSession,
    remote_stub::RemoteStubSession,
    shared::{
        messages::{
            ParticipantLogMessage,
            ParticipantMessage,
        },
        run_participant_runtime,
        ParticipantDriverSession,
        ParticipantLaunchSpec,
        ResolvedFrontendKind,
    },
};
use chrono::Utc;
use client_simulator_config::{
    Config,
    ParticipantBackendKind,
    ParticipantConfig,
};
use eyre::{
    OptionExt as _,
    Result,
};
use std::sync::Arc;
use tokio::{
    sync::{
        mpsc::{
            unbounded_channel,
            UnboundedReceiver,
            UnboundedSender,
        },
        watch,
    },
    time::{
        timeout,
        Duration,
    },
};
use tokio_util::sync::{
    CancellationToken,
    DropGuard,
};

mod cloudflare;
mod local;
mod remote_stub;
pub mod shared;

pub use shared::{
    ParticipantState,
    ParticipantStore,
};

/// Handle to a participant session managed by the participant runtime.
#[derive(Debug, Clone)]
pub struct Participant {
    pub name: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub state: watch::Receiver<ParticipantState>,
    _participant_task_guard: Arc<DropGuard>,
    sender: UnboundedSender<ParticipantMessage>,
}

impl PartialEq for Participant {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Participant {
    pub fn spawn_with_app_config(config: &Config, cookie_manager: HyperSessionCookieManger) -> Result<Self> {
        let (participant, _) = Self::spawn_with_app_config_and_receiver(config, cookie_manager)?;
        Ok(participant)
    }

    pub fn spawn(config: &Config, cookie_manager: HyperSessionCookieManger) -> Result<Self> {
        match config.backend {
            ParticipantBackendKind::Local => Self::spawn_with_app_config(config, cookie_manager),
            ParticipantBackendKind::Cloudflare => Self::spawn_cloudflare(config, cookie_manager),
            ParticipantBackendKind::RemoteStub => Self::spawn_remote_stub(config, cookie_manager),
        }
    }

    pub fn spawn_with_app_config_and_receiver(
        config: &Config,
        cookie_manager: HyperSessionCookieManger,
    ) -> Result<(Self, UnboundedReceiver<ParticipantLogMessage>)> {
        let session_url = config.url.clone().ok_or_eyre("No session URL provided in the config")?;
        let base_url = session_url.origin().unicode_serialization();
        let cookie = cookie_manager.give_cookie(&base_url);
        let name = cookie.as_ref().map(BorrowedCookie::username);
        let participant_config = ParticipantConfig::new(config, name)?;
        debug!("Participant config: {:#?}", participant_config);
        Self::with_participant_config(participant_config, cookie, cookie_manager)
    }

    pub fn with_participant_config(
        participant_config: ParticipantConfig,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Result<(Self, UnboundedReceiver<ParticipantLogMessage>)> {
        let launch_spec = ParticipantLaunchSpec::from(participant_config.clone());
        let browser_config = client_simulator_config::BrowserConfig::from(&participant_config);
        let name = launch_spec.username.clone();

        let (sender_tx, receiver_tx) = unbounded_channel::<ParticipantMessage>();
        let (sender_rx, receiver_rx) = unbounded_channel::<ParticipantLogMessage>();

        let (state_receiver, task_guard) = spawn_session(
            name.clone(),
            receiver_tx,
            sender_rx.clone(),
            LocalChromiumSession::new(launch_spec, browser_config, sender_rx.clone(), cookie, cookie_manager),
        );

        Ok((
            Self {
                name,
                created: chrono::Utc::now(),
                state: state_receiver,
                _participant_task_guard: task_guard,
                sender: sender_tx,
            },
            receiver_rx,
        ))
    }

    pub fn spawn_remote_stub(config: &Config, cookie_manager: HyperSessionCookieManger) -> Result<Self> {
        let session_url = config.url.clone().ok_or_eyre("No session URL provided in the config")?;
        let base_url = session_url.origin().unicode_serialization();
        let cookie = cookie_manager.give_cookie(&base_url);
        let name = cookie.as_ref().map(BorrowedCookie::username);
        let participant_config = ParticipantConfig::new(config, name)?;
        let launch_spec = ParticipantLaunchSpec::from(participant_config);
        let name = launch_spec.username.clone();

        let (sender, receiver) = unbounded_channel::<ParticipantMessage>();
        let (log_sender, _log_receiver) = unbounded_channel::<ParticipantLogMessage>();
        let (state_receiver, task_guard) = spawn_session(
            name.clone(),
            receiver,
            log_sender.clone(),
            RemoteStubSession::new(launch_spec, log_sender),
        );

        Ok(Self {
            name,
            created: Utc::now(),
            state: state_receiver,
            _participant_task_guard: task_guard,
            sender,
        })
    }

    pub fn spawn_cloudflare(config: &Config, cookie_manager: HyperSessionCookieManger) -> Result<Self> {
        let session_url = config.url.clone().ok_or_eyre("No session URL provided in the config")?;
        let frontend_kind = ResolvedFrontendKind::from_session_url(&session_url);
        let base_url = session_url.origin().unicode_serialization();
        let cookie = matches!(frontend_kind, ResolvedFrontendKind::HyperCore)
            .then(|| cookie_manager.give_cookie(&base_url))
            .flatten();
        let name = cookie.as_ref().map(BorrowedCookie::username);
        let participant_config = ParticipantConfig::new(config, name)?;
        let launch_spec = ParticipantLaunchSpec::from(participant_config);
        let name = launch_spec.username.clone();

        let (sender, receiver) = unbounded_channel::<ParticipantMessage>();
        let (log_sender, _log_receiver) = unbounded_channel::<ParticipantLogMessage>();
        let (state_receiver, task_guard) = spawn_session(
            name.clone(),
            receiver,
            log_sender.clone(),
            cloudflare::CloudflareSession::new(
                launch_spec,
                cloudflare::CloudflareLaunchOptions::from(config),
                config.cloudflare.clone(),
                log_sender,
                cookie,
                cookie_manager,
            ),
        );

        Ok(Self {
            name,
            created: Utc::now(),
            state: state_receiver,
            _participant_task_guard: task_guard,
            sender,
        })
    }
}

fn spawn_session<S>(
    name: String,
    receiver: UnboundedReceiver<ParticipantMessage>,
    log_sender: UnboundedSender<ParticipantLogMessage>,
    session: S,
) -> (watch::Receiver<ParticipantState>, Arc<DropGuard>)
where
    S: ParticipantDriverSession + 'static,
{
    let task_cancellation_token = CancellationToken::new();
    let task_cancellation_guard = task_cancellation_token.clone().drop_guard();
    let (state_sender, state_receiver) = watch::channel(Default::default());
    let task_sender_for_task = log_sender.clone();

    tokio::task::spawn(async move {
        let result = run_participant_runtime(
            receiver,
            log_sender.clone(),
            state_sender,
            session,
            task_cancellation_token.clone(),
        )
        .await;

        if let Err(err) = result {
            error!(participant = %name, "Failed to create participant: {err}");
            let _ = task_sender_for_task.send(ParticipantLogMessage::new(
                "error",
                &name,
                format!("Failed to create participant: {err}"),
            ));
        }

        debug!(participant = %name, "Participant task canceled");
        let _ = task_sender_for_task.send(ParticipantLogMessage::new(
            "debug",
            &name,
            format!("Participant {name} has been closed"),
        ));
    });

    (state_receiver, Arc::new(task_cancellation_guard))
}

impl Participant {
    pub async fn close(mut self) {
        let initial_state = self.state.borrow().clone();
        if !initial_state.running {
            debug!(participant = %self.name, "Already closed the browser");
            return;
        }

        if initial_state.joined {
            if self.sender.send(ParticipantMessage::Leave).is_err() {
                error!(participant = %self.name, "Was not able to send ParticipantMessage::Leave message");
            } else {
                match timeout(
                    Duration::from_secs(5),
                    self.state.wait_for(|state| !state.running || !state.joined),
                )
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => {
                        error!(participant = %self.name, "Failed to wait for participant to leave: {err}");
                    }
                    Err(_) => {
                        warn!(participant = %self.name, "Timed out waiting for participant to leave before closing");
                    }
                }
            }
        }

        if self.sender.send(ParticipantMessage::Close).is_ok() {
            match timeout(Duration::from_secs(10), self.state.wait_for(|state| !state.running)).await {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => {
                    error!(participant = %self.name, "Failed to wait for participant to close: {err}");
                }
                Err(_) => {
                    warn!(participant = %self.name, "Timed out waiting for participant to close");
                }
            }
        } else {
            error!(participant = %self.name, "Was not able to send ParticipantMessage::Close message");
        }
    }

    pub fn join(&self) {
        let state = self.state.borrow();
        if !state.running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if state.joined {
            debug!(self.name, "Already joined");
            return;
        }
        if self.sender.send(ParticipantMessage::Join).is_err() {
            error!("Was not able to send ParticipantMessage::Join message")
        }
    }

    pub fn send_message(&self, message: ParticipantMessage) {
        if let ParticipantMessage::Join = &message {
            return self.join();
        }

        let state = self.state.borrow();
        if !state.running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if !state.joined {
            debug!(self.name, "Cannot send message {}, not in the space yet", &message);
            return;
        }
        if self.sender.send(message.clone()).is_err() {
            error!("Was not able to send message: {message}")
        }

        debug!("Sent message {message:?}");
    }

    pub fn leave(&self) {
        self.send_message(ParticipantMessage::Leave);
    }

    pub fn toggle_audio(&self) {
        self.send_message(ParticipantMessage::ToggleAudio);
    }

    pub fn toggle_video(&self) {
        self.send_message(ParticipantMessage::ToggleVideo);
    }

    pub fn toggle_screen_share(&self) {
        self.send_message(ParticipantMessage::ToggleScreenshare);
    }

    pub fn toggle_screenshare(&self) {
        self.toggle_screen_share();
    }

    pub fn toggle_auto_gain_control(&self) {
        self.send_message(ParticipantMessage::ToggleAutoGainControl);
    }

    pub fn set_noise_suppression(&self, value: client_simulator_config::NoiseSuppression) {
        self.send_message(ParticipantMessage::SetNoiseSuppression(value));
    }

    pub fn set_webcam_resolutions(&self, value: client_simulator_config::WebcamResolution) {
        self.send_message(ParticipantMessage::SetWebcamResolutions(value));
    }

    pub fn set_webcam_resolution(&self, value: client_simulator_config::WebcamResolution) {
        self.set_webcam_resolutions(value);
    }

    pub fn toggle_background_blur(&self) {
        self.send_message(ParticipantMessage::ToggleBackgroundBlur);
    }
}
