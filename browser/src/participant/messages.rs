use client_simulator_config::{
    NoiseSuppression,
    WebcamResolution,
};
use derive_more::Display;

#[derive(Clone, Display)]
pub enum ParticipantMessage {
    Join,
    Leave,
    Close,
    ToggleAudio,
    ToggleVideo,
    SetNoiseSuppression(NoiseSuppression),
    SetWebcamResolutions(WebcamResolution),
    ToggleBackgroundBlur,
}
