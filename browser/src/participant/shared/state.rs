use client_simulator_config::{
    NoiseSuppression,
    TransportMode,
    VideoConstraint,
};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantState {
    pub username: String,
    pub running: bool,
    pub joined: bool,
    pub muted: bool,
    pub video_activated: bool,
    pub auto_gain_control: bool,
    pub noise_suppression: NoiseSuppression,
    pub transport_mode: TransportMode,
    pub video_constraint_publish_webcam: VideoConstraint,
    pub video_constraint_subscribe: VideoConstraint,
    pub video_max_concurrent_tracks: Option<usize>,
    pub background_blur: bool,
    pub screenshare_activated: bool,
}
