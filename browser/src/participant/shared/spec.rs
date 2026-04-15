use client_simulator_config::{
    NoiseSuppression,
    ParticipantConfig,
    TransportMode,
    WebcamResolution,
};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::participant) enum ResolvedFrontendKind {
    HyperCore,
    HyperLite,
}

impl ResolvedFrontendKind {
    pub(in crate::participant) fn from_session_url(session_url: &Url) -> Self {
        let path = session_url.path();
        if path == "/m" || path.starts_with("/m/") {
            Self::HyperLite
        } else {
            Self::HyperCore
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::participant) struct ParticipantSettings {
    pub(in crate::participant) audio_enabled: bool,
    pub(in crate::participant) video_enabled: bool,
    pub(in crate::participant) screenshare_enabled: bool,
    pub(in crate::participant) noise_suppression: NoiseSuppression,
    pub(in crate::participant) transport: TransportMode,
    pub(in crate::participant) resolution: WebcamResolution,
    pub(in crate::participant) blur: bool,
}

impl From<&ParticipantConfig> for ParticipantSettings {
    fn from(config: &ParticipantConfig) -> Self {
        let app_config = &config.app_config;
        Self {
            audio_enabled: app_config.audio_enabled,
            video_enabled: app_config.video_enabled,
            screenshare_enabled: app_config.screenshare_enabled,
            noise_suppression: app_config.noise_suppression,
            transport: app_config.transport,
            resolution: app_config.resolution,
            blur: app_config.blur,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::participant) struct ParticipantLaunchSpec {
    pub(in crate::participant) username: String,
    pub(in crate::participant) session_url: Url,
    pub(in crate::participant) frontend_kind: ResolvedFrontendKind,
    pub(in crate::participant) settings: ParticipantSettings,
}

impl ParticipantLaunchSpec {
    pub(in crate::participant) fn base_url(&self) -> Url {
        let mut url = self.session_url.clone();
        url.set_path("/");
        url
    }
}

impl From<ParticipantConfig> for ParticipantLaunchSpec {
    fn from(config: ParticipantConfig) -> Self {
        Self {
            username: config.username.clone(),
            session_url: config.session_url.clone(),
            frontend_kind: ResolvedFrontendKind::from_session_url(&config.session_url),
            settings: ParticipantSettings::from(&config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ParticipantLaunchSpec,
        ResolvedFrontendKind,
    };
    use client_simulator_config::{
        Config,
        NoiseSuppression,
        ParticipantConfig,
        TransportMode,
        WebcamResolution,
    };
    use url::Url;

    #[test]
    fn converts_participant_config_to_launch_spec() {
        let participant_config = ParticipantConfig {
            username: "robert".to_string(),
            session_url: Url::parse("https://example.com/m/demo").unwrap(),
            app_config: Config {
                audio_enabled: true,
                video_enabled: false,
                screenshare_enabled: true,
                noise_suppression: NoiseSuppression::RNNoise,
                transport: TransportMode::WebRTC,
                resolution: WebcamResolution::P720,
                blur: true,
                ..Default::default()
            },
        };

        let spec = ParticipantLaunchSpec::from(participant_config);

        assert_eq!(spec.username, "robert");
        assert_eq!(spec.session_url.as_str(), "https://example.com/m/demo");
        assert_eq!(spec.frontend_kind, ResolvedFrontendKind::HyperLite);
        assert!(spec.settings.audio_enabled);
        assert!(!spec.settings.video_enabled);
        assert!(spec.settings.screenshare_enabled);
        assert_eq!(spec.settings.noise_suppression, NoiseSuppression::RNNoise);
        assert_eq!(spec.settings.transport, TransportMode::WebRTC);
        assert_eq!(spec.settings.resolution, WebcamResolution::P720);
        assert!(spec.settings.blur);
    }
}
