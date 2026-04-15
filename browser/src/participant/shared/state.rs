use client_simulator_config::{
    NoiseSuppression,
    TransportMode,
    WebcamResolution,
};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantState {
    pub username: String,
    pub running: bool,
    pub joined: bool,
    pub muted: bool,
    pub video_activated: bool,
    pub noise_suppression: NoiseSuppression,
    pub transport_mode: TransportMode,
    pub webcam_resolution: WebcamResolution,
    pub background_blur: bool,
    pub screenshare_activated: bool,
}
