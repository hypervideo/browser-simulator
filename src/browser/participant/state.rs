use crate::config::{
    NoiseSuppression,
    TransportMode,
    WebcamResolution,
};

#[derive(Debug, Default, Clone)]
pub struct ParticipantState {
    pub running: bool,
    pub joined: bool,
    pub muted: bool,
    pub video_activated: bool,
    pub noise_suppression: NoiseSuppression,
    pub transport_mode: TransportMode,
    pub webcam_resolution: WebcamResolution,
    pub background_blur: bool,
}
