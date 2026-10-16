use serde::{
    Deserialize,
    Serialize,
};
use strum::{
    Display,
    EnumIter,
    EnumString,
};

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum TransportMode {
    #[default]
    WebTransport,
    WebRTC,
}

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, Serialize, Deserialize, PartialEq, Eq)]
pub enum VideoConstraint {
    #[default]
    #[strum(to_string = "none")]
    #[serde(rename = "none")]
    None,
    #[strum(to_string = "90p")]
    #[serde(rename = "90p")]
    P90,
    #[strum(to_string = "144p")]
    #[serde(rename = "144p")]
    P144,
    #[strum(to_string = "240p")]
    #[serde(rename = "240p")]
    P240,
    #[strum(to_string = "360p")]
    #[serde(rename = "360p")]
    P360,
    #[strum(to_string = "480p")]
    #[serde(rename = "480p")]
    P480,
    #[strum(to_string = "720p")]
    #[serde(rename = "720p")]
    P720,
    #[strum(to_string = "1080p")]
    #[serde(rename = "1080p")]
    P1080,
    #[strum(to_string = "1440p")]
    #[serde(rename = "1440p")]
    P1440,
    #[strum(to_string = "2160p")]
    #[serde(rename = "2160p")]
    P2160,
}

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, PartialEq, Eq)]
pub enum VideoMaxConcurrentTracksPreset {
    #[default]
    #[strum(to_string = "unlimited")]
    Unlimited,
    #[strum(to_string = "1")]
    One,
    #[strum(to_string = "2")]
    Two,
    #[strum(to_string = "3")]
    Three,
    #[strum(to_string = "4")]
    Four,
}

impl VideoMaxConcurrentTracksPreset {
    pub const fn to_option(self) -> Option<usize> {
        match self {
            Self::Unlimited => None,
            Self::One => Some(1),
            Self::Two => Some(2),
            Self::Three => Some(3),
            Self::Four => Some(4),
        }
    }

    pub const fn from_option(value: Option<usize>) -> Self {
        match value {
            None => Self::Unlimited,
            Some(1) => Self::One,
            Some(2) => Self::Two,
            Some(3) => Self::Three,
            Some(4) => Self::Four,
            // Off-list values (e.g. set via config/CLI) have no preset; the TUI
            // selector falls back to Unlimited, while the table shows the real value.
            Some(_) => Self::Unlimited,
        }
    }
}

/// A value of hyper.video's noise suppression setting.
#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum NoiseSuppression {
    #[default]
    #[strum(to_string = "none")]
    #[serde(rename = "none")]
    Disabled,
    #[strum(to_string = "hyper-voice")]
    #[serde(rename = "hyper-voice")]
    HyperVoice,
}

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ParticipantBackendKind {
    #[default]
    Local,
    Cloudflare,
    RemoteStub,
    AwsDeviceFarm,
}

impl ParticipantBackendKind {
    pub const fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NoiseSuppression,
        ParticipantBackendKind,
        VideoConstraint,
        VideoMaxConcurrentTracksPreset,
    };
    use std::str::FromStr as _;

    #[test]
    fn video_constraint_round_trips_hyper_media_setting_names() {
        let values = [
            ("none", VideoConstraint::None),
            ("90p", VideoConstraint::P90),
            ("144p", VideoConstraint::P144),
            ("240p", VideoConstraint::P240),
            ("360p", VideoConstraint::P360),
            ("480p", VideoConstraint::P480),
            ("720p", VideoConstraint::P720),
            ("1080p", VideoConstraint::P1080),
            ("1440p", VideoConstraint::P1440),
            ("2160p", VideoConstraint::P2160),
        ];

        for (raw, expected) in values {
            assert_eq!(VideoConstraint::from_str(raw).unwrap(), expected);
            assert_eq!(expected.to_string(), raw);
            assert_eq!(serde_json::to_value(expected).unwrap(), raw);
            assert_eq!(serde_json::from_value::<VideoConstraint>(raw.into()).unwrap(), expected);
        }
    }

    #[test]
    fn noise_suppression_round_trips_hyper_media_setting_names() {
        let values = [
            ("none", NoiseSuppression::Disabled),
            ("hyper-voice", NoiseSuppression::HyperVoice),
        ];

        for (raw, expected) in values {
            assert_eq!(NoiseSuppression::from_str(raw).unwrap(), expected);
            assert_eq!(expected.to_string(), raw);
            assert_eq!(serde_json::to_value(expected).unwrap(), raw);
            assert_eq!(
                serde_json::from_value::<NoiseSuppression>(raw.into()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn video_track_limit_presets_convert_to_nullable_track_counts() {
        assert_eq!(VideoMaxConcurrentTracksPreset::Unlimited.to_option(), None);
        assert_eq!(VideoMaxConcurrentTracksPreset::One.to_option(), Some(1));
        assert_eq!(VideoMaxConcurrentTracksPreset::Two.to_option(), Some(2));
        assert_eq!(VideoMaxConcurrentTracksPreset::Three.to_option(), Some(3));
        assert_eq!(VideoMaxConcurrentTracksPreset::Four.to_option(), Some(4));
        assert_eq!(
            VideoMaxConcurrentTracksPreset::from_option(None),
            VideoMaxConcurrentTracksPreset::Unlimited
        );
        assert_eq!(
            VideoMaxConcurrentTracksPreset::from_option(Some(1)),
            VideoMaxConcurrentTracksPreset::One
        );
        assert_eq!(
            VideoMaxConcurrentTracksPreset::from_option(Some(4)),
            VideoMaxConcurrentTracksPreset::Four
        );
        // Off-list live values fall back to Unlimited for TUI preselection only;
        // the participants table displays the real number separately.
        assert_eq!(
            VideoMaxConcurrentTracksPreset::from_option(Some(8)),
            VideoMaxConcurrentTracksPreset::Unlimited
        );
    }

    #[test]
    fn aws_device_farm_round_trips_kebab_case() {
        let kind = ParticipantBackendKind::from_str("aws-device-farm").unwrap();
        assert_eq!(kind, ParticipantBackendKind::AwsDeviceFarm);
        assert_eq!(kind.to_string(), "aws-device-farm");
        assert!(!kind.is_local());
    }
}
