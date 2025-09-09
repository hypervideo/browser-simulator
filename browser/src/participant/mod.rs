use super::auth::{
    BorrowedCookie,
    HyperSessionCookieManger,
    HyperSessionCookieStash,
};
use crate::participant::{
    messages::ParticipantLogMessage,
    remote::spawn_remote,
    transport_data::ParticipantConfigQuery,
};
use chrono::Utc;
use client_simulator_config::{
    Config,
    NoiseSuppression,
    ParticipantConfig,
    WebcamResolution,
};
use eyre::{
    OptionExt as _,
    Result,
};
use messages::ParticipantMessage;
use std::sync::Arc;
use tokio::sync::{
    mpsc::{
        unbounded_channel,
        UnboundedReceiver,
        UnboundedSender,
    },
    watch,
};
use tokio_util::sync::{
    CancellationToken,
    DropGuard,
};

mod commands;
mod inner;
mod inner_lite;
pub mod messages;
mod remote;
mod selectors;
mod state;
mod store;
pub mod transport_data;

use inner::ParticipantInner;
pub use state::ParticipantState;
pub use store::ParticipantStore;

/// Participant that will spawn a browser and join the given space from the config.
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

    pub fn spawn_with_app_config_and_receiver(
        config: &Config,
        cookie_manager: HyperSessionCookieManger,
    ) -> Result<(Self, UnboundedReceiver<ParticipantLogMessage>)> {
        let session_url = config.url.clone().ok_or_eyre("No session URL provided in the config")?;
        let base_url = session_url.origin().unicode_serialization();
        let cookie = cookie_manager.give_cookie(&base_url);
        let name = cookie.as_ref().map(|c| c.username());
        let participant_config = ParticipantConfig::new(config, name)?;
        debug!("Participant config: {:#?}", participant_config);
        Self::with_participant_config(participant_config, cookie, cookie_manager)
    }

    pub fn with_participant_config(
        participant_config: ParticipantConfig,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Result<(Self, UnboundedReceiver<ParticipantLogMessage>)> {
        let (sender_tx, receiver_tx) = unbounded_channel::<ParticipantMessage>();
        let (sender_rx, receiver_rx) = unbounded_channel::<ParticipantLogMessage>();

        let name = participant_config.username.clone();
        let task_cancellation_token = CancellationToken::new();
        let task_cancellation_guard = task_cancellation_token.clone().drop_guard();
        let (state_sender, state_receiver) = watch::channel(Default::default());

        tokio::task::spawn({
            let name = name.clone();
            let task_sender_for_worker = sender_rx.clone();
            let task_sender_for_task = sender_rx.clone();
            async move {
                tokio::select! {
                    biased;
                    _ = task_cancellation_token.cancelled() => {},

                    result = async move {
                        if participant_config.is_lite_frontend() {
                            inner_lite::ParticipantInnerLite::run(
                                participant_config,
                                receiver_tx,
                                task_sender_for_worker,
                                state_sender,
                            ).await
                        } else {
                            ParticipantInner::run(
                                participant_config,
                                cookie,
                                cookie_manager,
                                receiver_tx,
                                task_sender_for_worker,
                                state_sender,
                            ).await
                        }
                    } => {
                        if let Err(err) = result {
                            error!(?name, "Failed to create participant: {err}");
                            let _ = task_sender_for_task.send(ParticipantLogMessage::new("error", &name, format!("Failed to create participant: {err}")));
                        }
                    }
                };

                debug!(?name, "Participant task canceled");
                let _ = task_sender_for_task.send(ParticipantLogMessage::new(
                    "debug",
                    &name,
                    format!("Participant {name} has been closed"),
                ));
            }
        });

        Ok((
            Self {
                name,
                created: chrono::Utc::now(),
                state: state_receiver,
                _participant_task_guard: Arc::new(task_cancellation_guard),
                sender: sender_tx,
            },
            receiver_rx,
        ))
    }

    pub fn spawn_remote(config: &Config, cookie_manager: HyperSessionCookieManger) -> Result<Self> {
        let session_url = config.url.clone().ok_or_eyre("No session URL provided in the config")?;
        let base_url = session_url.origin().unicode_serialization();
        let cookie = cookie_manager.give_cookie(&base_url);
        let query = ParticipantConfigQuery::new(config, cookie.as_ref())?;
        let name = query.username.clone();

        let (sender, receiver) = unbounded_channel::<ParticipantMessage>();
        let task_cancellation_token = CancellationToken::new();
        let task_cancellation_guard = task_cancellation_token.clone().drop_guard();
        let (state_sender, state_receiver) = watch::channel(Default::default());

        tokio::task::spawn({
            async move {
                tokio::select! {
                    biased;
                    _ = task_cancellation_token.cancelled() => {},

                    result = spawn_remote(receiver, state_sender, query, cookie, cookie_manager) => {
                        if let Err(err) = result {
                            error!("Failed to spawn remote participant: {err}");
                        }
                    }
                };

                debug!("Remote participant task canceled");
            }
        });

        Ok(Self {
            name,
            created: Utc::now(),
            state: state_receiver,
            _participant_task_guard: Arc::new(task_cancellation_guard),
            sender,
        })
    }
}

impl Participant {
    pub async fn close(mut self) {
        if !self.state.borrow().running {
            debug!(self.name, "Already closed the browser");
            return;
        }
        if self.sender.send(ParticipantMessage::Close).is_ok() {
            if let Err(err) = self.state.wait_for(|state| !state.running).await {
                error!("Failed to wait for participant to close: {err}");
            };
        } else {
            error!("Was not able to send ParticipantMessage::Close message")
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

    pub fn set_noise_suppression(&self, value: NoiseSuppression) {
        self.send_message(ParticipantMessage::SetNoiseSuppression(value));
    }

    pub fn set_webcam_resolutions(&self, value: WebcamResolution) {
        self.send_message(ParticipantMessage::SetWebcamResolutions(value));
    }

    pub fn toggle_background_blur(&self) {
        self.send_message(ParticipantMessage::ToggleBackgroundBlur);
    }
}
