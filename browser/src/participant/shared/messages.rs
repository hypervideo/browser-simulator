use client_simulator_config::{
    NoiseSuppression,
    VideoConstraint,
};
use std::fmt;

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub enum ParticipantMessage {
    Join,
    Leave,
    Close,
    ToggleAudio,
    ToggleVideo,
    ToggleScreenshare,
    ToggleAutoGainControl,
    SetNoiseSuppression(NoiseSuppression),
    SetVideoConstraintPublishWebcam(VideoConstraint),
    SetVideoConstraintSubscribe(VideoConstraint),
    SetVideoMaxConcurrentTracks(Option<usize>),
    ToggleBackgroundBlur,
}

impl fmt::Display for ParticipantMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantLogMessage {
    pub username: String,
    pub level: String,
    pub message: String,
}

impl ParticipantLogMessage {
    pub fn new(level: &str, username: &str, message: impl ToString) -> Self {
        Self {
            username: username.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        }
    }

    pub fn write(&self) {
        match self.level.as_str() {
            "trace" => trace!(self.username, "{}", self.message),
            "debug" => debug!(self.username, "{}", self.message),
            "info" => info!(self.username, "{}", self.message),
            "warn" => warn!(self.username, "{}", self.message),
            "error" => error!(self.username, "{}", self.message),
            _ => warn!(
                self.username,
                "Received unexpected log level {} with message: {}", self.level, self.message
            ),
        }
    }
}
