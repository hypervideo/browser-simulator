use serde::{
    de::{
        self,
        IntoDeserializer as _,
    },
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};
use std::{
    fmt,
    marker::PhantomData,
    str::FromStr,
};
use strum::{
    Display,
    EnumIter,
    EnumString,
    IntoEnumIterator as _,
};

/// A participant's own value of one of hyper.video's settings, or none, so that hyper.video decides.
///
/// A Hyper Core participant sets its own value in the participant layer of hyper.video's settings, and unsets the
/// setting there for [`Self::Default`], so that an admin's value for the session applies, else hyper.video's default.
/// The CLI, the config and `--participant` JSON spell [`Self::Default`] as `default`, and an own value as `T` does.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum OwnOrDefault<T> {
    /// No value of the participant's own.
    #[default]
    Default,
    /// The participant's own value.
    Own(T),
}

impl<T> OwnOrDefault<T> {
    const DEFAULT: &str = "default";

    /// The participant's own value, `None` for [`Self::Default`].
    pub fn own(self) -> Option<T> {
        match self {
            Self::Default => None,
            Self::Own(value) => Some(value),
        }
    }

    /// The setting with `f` applied to the participant's own value.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> OwnOrDefault<U> {
        match self {
            Self::Default => OwnOrDefault::Default,
            Self::Own(value) => OwnOrDefault::Own(f(value)),
        }
    }
}

impl<T: fmt::Display> fmt::Display for OwnOrDefault<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str(Self::DEFAULT),
            Self::Own(value) => value.fmt(f),
        }
    }
}

impl<T> FromStr for OwnOrDefault<T>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == Self::DEFAULT {
            return Ok(Self::Default);
        }
        value
            .parse()
            .map(Self::Own)
            .map_err(|error| format!("neither `{}` nor a value of the setting: {error}", Self::DEFAULT))
    }
}

impl<T: Serialize> Serialize for OwnOrDefault<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Default => serializer.serialize_str(Self::DEFAULT),
            Self::Own(value) => value.serialize(serializer),
        }
    }
}

/// Reads `default`, and the `null` of an empty config key, as [`OwnOrDefault::Default`], another string as
/// [`OwnOrDefault`]'s [`FromStr`] does, and a bool or a number as `T` reads it.
impl<'de, T> Deserialize<'de> for OwnOrDefault<T>
where
    T: Deserialize<'de> + FromStr,
    T::Err: fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(PhantomData<T>);

        impl<'de, T> de::Visitor<'de> for Visitor<T>
        where
            T: Deserialize<'de> + FromStr,
            T::Err: fmt::Display,
        {
            type Value = OwnOrDefault<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "`{}` or a value of the setting", OwnOrDefault::<T>::DEFAULT)
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(OwnOrDefault::Default)
            }

            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(OwnOrDefault::Default)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(E::custom)
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                T::deserialize(value.into_deserializer()).map(OwnOrDefault::Own)
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                T::deserialize(value.into_deserializer()).map(OwnOrDefault::Own)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                T::deserialize(value.into_deserializer()).map(OwnOrDefault::Own)
            }
        }

        deserializer.deserialize_any(Visitor(PhantomData))
    }
}

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
    #[strum(to_string = "4320p")]
    #[serde(rename = "4320p")]
    P4320,
}

impl VideoConstraint {
    /// The `VideoResolutionLevel` of hyper.video's settings that limits video to the constraint, `None` for no limit,
    /// which the settings spell `null`.
    pub const fn resolution_level(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::P90 => Some("P90"),
            Self::P144 => Some("P144"),
            Self::P240 => Some("P240"),
            Self::P360 => Some("P360"),
            Self::P480 => Some("P480"),
            Self::P720 => Some("P720"),
            Self::P1080 => Some("P1080"),
            Self::P1440 => Some("P1440"),
            Self::P2160 => Some("P2160"),
            Self::P4320 => Some("P4320"),
        }
    }

    /// The constraint whose [`Self::resolution_level`] is `level`, `None` for a level that no constraint has.
    pub fn from_resolution_level(level: Option<&str>) -> Option<Self> {
        Self::iter().find(|constraint| constraint.resolution_level() == level)
    }
}

/// The most video tracks that a stream encodes at once.
///
/// The CLI and the config spell it `unlimited` or as a number of tracks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum VideoMaxConcurrentTracks {
    /// Every track of the stream, also what a participant's state holds before it reads the limit.
    #[default]
    Unlimited,
    /// At most this many tracks.
    Limit(usize),
}

impl VideoMaxConcurrentTracks {
    const UNLIMITED: &str = "unlimited";

    /// The limit as hyper.video's `videoSendMaxConcurrentTracks` setting takes it, `None` for unlimited, which the
    /// setting spells `null`.
    pub const fn limit(self) -> Option<usize> {
        match self {
            Self::Unlimited => None,
            Self::Limit(tracks) => Some(tracks),
        }
    }
}

impl fmt::Display for VideoMaxConcurrentTracks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlimited => f.write_str(Self::UNLIMITED),
            Self::Limit(tracks) => write!(f, "{tracks}"),
        }
    }
}

impl FromStr for VideoMaxConcurrentTracks {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == Self::UNLIMITED {
            return Ok(Self::Unlimited);
        }
        value
            .parse()
            .map(Self::Limit)
            .map_err(|_| format!("expected `{}` or a number of tracks, not `{value}`", Self::UNLIMITED))
    }
}

impl Serialize for VideoMaxConcurrentTracks {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Unlimited => serializer.serialize_str(Self::UNLIMITED),
            Self::Limit(tracks) => serializer.serialize_u64(*tracks as u64),
        }
    }
}

impl<'de> Deserialize<'de> for VideoMaxConcurrentTracks {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl de::Visitor<'_> for Visitor {
            type Value = VideoMaxConcurrentTracks;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "`{}` or a number of tracks", VideoMaxConcurrentTracks::UNLIMITED)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                usize::try_from(value)
                    .map(VideoMaxConcurrentTracks::Limit)
                    .map_err(|_| E::invalid_value(de::Unexpected::Unsigned(value), &self))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                usize::try_from(value)
                    .map(VideoMaxConcurrentTracks::Limit)
                    .map_err(|_| E::invalid_value(de::Unexpected::Signed(value), &self))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, PartialEq, Eq)]
pub enum VideoMaxConcurrentTracksPreset {
    /// No limit of the participant's own, so hyper.video's default applies.
    #[default]
    #[strum(to_string = "default")]
    Default,
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
    /// The track limit that the preset picks.
    pub const fn to_setting(self) -> OwnOrDefault<VideoMaxConcurrentTracks> {
        match self {
            Self::Default => OwnOrDefault::Default,
            Self::Unlimited => OwnOrDefault::Own(VideoMaxConcurrentTracks::Unlimited),
            Self::One => OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(1)),
            Self::Two => OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(2)),
            Self::Three => OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(3)),
            Self::Four => OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(4)),
        }
    }

    pub const fn from_setting(value: OwnOrDefault<VideoMaxConcurrentTracks>) -> Self {
        match value {
            OwnOrDefault::Default => Self::Default,
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Unlimited) => Self::Unlimited,
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(1)) => Self::One,
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(2)) => Self::Two,
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(3)) => Self::Three,
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(4)) => Self::Four,
            // Off-list values (e.g. set via config/CLI) have no preset; the TUI
            // selector falls back to Unlimited, while the table shows the real value.
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(_)) => Self::Unlimited,
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
        OwnOrDefault,
        ParticipantBackendKind,
        VideoConstraint,
        VideoMaxConcurrentTracks,
        VideoMaxConcurrentTracksPreset,
    };
    use std::str::FromStr as _;
    use strum::IntoEnumIterator as _;

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
            ("4320p", VideoConstraint::P4320),
        ];

        for (raw, expected) in values {
            assert_eq!(VideoConstraint::from_str(raw).unwrap(), expected);
            assert_eq!(expected.to_string(), raw);
            assert_eq!(serde_json::to_value(expected).unwrap(), raw);
            assert_eq!(serde_json::from_value::<VideoConstraint>(raw.into()).unwrap(), expected);
        }
    }

    #[test]
    fn video_constraints_round_trip_through_resolution_levels() {
        for constraint in VideoConstraint::iter() {
            assert_eq!(
                VideoConstraint::from_resolution_level(constraint.resolution_level()),
                Some(constraint)
            );
        }
        assert_eq!(
            VideoConstraint::from_resolution_level(Some("P4320")),
            Some(VideoConstraint::P4320)
        );
        assert_eq!(VideoConstraint::from_resolution_level(Some("P8640")), None);
    }

    /// A setting reads and writes `default` for hyper.video's default, and an own value as the setting spells it.
    #[test]
    fn own_or_default_reads_default_or_own_value() {
        assert_eq!(
            OwnOrDefault::<bool>::from_str("default").unwrap(),
            OwnOrDefault::Default
        );
        assert_eq!(OwnOrDefault::<bool>::from_str("true").unwrap(), OwnOrDefault::Own(true));
        assert_eq!(
            OwnOrDefault::<VideoConstraint>::from_str("none").unwrap(),
            OwnOrDefault::Own(VideoConstraint::None)
        );
        assert!(OwnOrDefault::<bool>::from_str("maybe").is_err());

        let values = [
            (serde_json::json!("default"), OwnOrDefault::Default),
            (
                serde_json::json!(2),
                OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(2)),
            ),
            (
                serde_json::json!("unlimited"),
                OwnOrDefault::Own(VideoMaxConcurrentTracks::Unlimited),
            ),
        ];
        for (json, expected) in values {
            assert_eq!(serde_json::to_value(expected).unwrap(), json);
            assert_eq!(
                serde_json::from_value::<OwnOrDefault<VideoMaxConcurrentTracks>>(json).unwrap(),
                expected
            );
        }
        assert_eq!(
            serde_json::from_value::<OwnOrDefault<bool>>(serde_json::json!(false)).unwrap(),
            OwnOrDefault::Own(false)
        );
        assert_eq!(
            serde_json::from_value::<OwnOrDefault<bool>>(serde_json::Value::Null).unwrap(),
            OwnOrDefault::Default
        );
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
    fn video_max_concurrent_tracks_reads_unlimited_or_track_count() {
        let values = [
            (
                "unlimited",
                serde_json::json!("unlimited"),
                VideoMaxConcurrentTracks::Unlimited,
            ),
            ("2", serde_json::json!(2), VideoMaxConcurrentTracks::Limit(2)),
        ];

        for (raw, json, expected) in values {
            assert_eq!(VideoMaxConcurrentTracks::from_str(raw).unwrap(), expected);
            assert_eq!(expected.to_string(), raw);
            assert_eq!(serde_json::to_value(expected).unwrap(), json);
            assert_eq!(
                serde_json::from_value::<VideoMaxConcurrentTracks>(json).unwrap(),
                expected
            );
            assert_eq!(
                serde_json::from_value::<VideoMaxConcurrentTracks>(raw.into()).unwrap(),
                expected
            );
        }
        assert!(VideoMaxConcurrentTracks::from_str("many").is_err());
        assert!(serde_json::from_value::<VideoMaxConcurrentTracks>(serde_json::json!(-1)).is_err());
    }

    #[test]
    fn video_track_limit_presets_convert_to_track_limits() {
        let limits = [
            (VideoMaxConcurrentTracksPreset::Default, OwnOrDefault::Default),
            (
                VideoMaxConcurrentTracksPreset::Unlimited,
                OwnOrDefault::Own(VideoMaxConcurrentTracks::Unlimited),
            ),
            (
                VideoMaxConcurrentTracksPreset::One,
                OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(1)),
            ),
            (
                VideoMaxConcurrentTracksPreset::Four,
                OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(4)),
            ),
        ];

        for (preset, limit) in limits {
            assert_eq!(preset.to_setting(), limit);
            assert_eq!(VideoMaxConcurrentTracksPreset::from_setting(limit), preset);
        }
        // Off-list live values fall back to Unlimited for TUI preselection only;
        // the participants table displays the real number separately.
        assert_eq!(
            VideoMaxConcurrentTracksPreset::from_setting(OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(8))),
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
