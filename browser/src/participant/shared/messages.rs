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

pub(in crate::participant) struct ParticipantLogMessage {
    participant: String,
    level: String,
    message: String,
}

impl ParticipantLogMessage {
    pub(in crate::participant) fn new(level: &str, participant: &str, message: impl ToString) -> Self {
        Self {
            participant: participant.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        }
    }

    pub(in crate::participant) fn write(&self) {
        match self.level.as_str() {
            "trace" => trace!(participant = %self.participant, "{}", self.message),
            "debug" => debug!(participant = %self.participant, "{}", self.message),
            "info" => info!(participant = %self.participant, "{}", self.message),
            "warn" => warn!(participant = %self.participant, "{}", self.message),
            "error" => error!(participant = %self.participant, "{}", self.message),
            _ => warn!(
                participant = %self.participant,
                "Received unexpected log level {} with message: {}", self.level, self.message
            ),
        }
    }
}
