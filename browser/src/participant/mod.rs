use super::auth::{
    BorrowedCookie,
    HyperSessionCookieManger,
    HyperSessionCookieStash,
};
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
mod messages;
mod state;
mod store;

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
        let session_url = config.url.clone().ok_or_eyre("No session URL provided in the config")?;
        let base_url = session_url.origin().unicode_serialization();
        let cookie = cookie_manager.give_cookie(&base_url);
        let name = cookie.as_ref().map(|c| c.username());
        let participant_config = ParticipantConfig::new(config, name)?;
        Self::with_participant_config(participant_config, cookie, cookie_manager)
    }

    pub fn with_participant_config(
        participant_config: ParticipantConfig,
        cookie: Option<BorrowedCookie>,
        cookie_manager: HyperSessionCookieManger,
    ) -> Result<Self> {
        let (sender, receiver) = unbounded_channel::<ParticipantMessage>();

        let name = participant_config.username.clone();
        let task_cancellation_token = CancellationToken::new();
        let task_cancellation_guard = task_cancellation_token.clone().drop_guard();
        let (state_sender, state_receiver) = watch::channel(Default::default());

        tokio::task::spawn({
            let name = name.clone();
            async move {
                tokio::select! {
                    biased;
                    _ = task_cancellation_token.cancelled() => {},

                    result = ParticipantInner::run(
                        participant_config,
                        cookie,
                        cookie_manager,
                        receiver,
                        state_sender,
                    ) => {
                        if let Err(err) = result {
                            error!(?name, "Failed to create participant: {err}")
                        }
                    }
                };

                debug!(?name, "Participant task canceled");
            }
        });

        Ok(Self {
            name,
            created: chrono::Utc::now(),
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

    fn send_message(&self, message: ParticipantMessage) {
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
