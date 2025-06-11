use derive_more::Display;

#[derive(Clone, Display)]
pub enum ParticipantMessage {
    Join,
    Leave,
    Close,
    ToggleAudio,
    ToggleVideo,
    ToggleTransportMode,
    ToggleNoiseSuppression,
    ToggleThroughWebcamResolutions,
    ToggleBackgroundBlur,
}
