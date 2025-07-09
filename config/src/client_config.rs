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

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum WebcamResolution {
    #[default]
    #[strum(to_string = "auto")]
    #[serde(rename = "auto")]
    Auto,
    P144,
    P240,
    P360,
    P480,
    P720,
    P1080,
    P1440,
    P2160,
    P4320,
}

#[derive(Debug, Default, Clone, Copy, Display, EnumIter, EnumString, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum NoiseSuppression {
    #[default]
    #[strum(to_string = "none")]
    #[serde(rename = "none")]
    Disabled,
    #[strum(to_string = "deepfilternet")]
    #[serde(rename = "deepfilternet")]
    Deepfilternet,
    #[strum(to_string = "rnnoise")]
    #[serde(rename = "rnnoise")]
    RNNoise,
    #[strum(to_string = "iris-shepherd")]
    #[serde(rename = "iris-shepherd")]
    IRISShepherd,
    #[strum(to_string = "krisp-high")]
    #[serde(rename = "krisp-high")]
    KrispHigh,
    #[strum(to_string = "krisp-medium")]
    #[serde(rename = "krisp-medium")]
    KrispMedium,
    #[strum(to_string = "krisp-low")]
    #[serde(rename = "krisp-low")]
    KrispLow,
    #[strum(to_string = "krisp-high-with-bvc")]
    #[serde(rename = "krisp-high-with-bvc")]
    KrispHighWithBVC,
    #[strum(to_string = "krisp-medium-with-bvc")]
    #[serde(rename = "krisp-medium-with-bvc")]
    KrispMediumWithBVC,
}
