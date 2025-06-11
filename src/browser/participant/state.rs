use derive_more::Display;

#[derive(Debug, Default, Clone, Display)]
pub enum TransportMode {
    #[default]
    WebTransport,
    WebRTC,
}

#[derive(Debug, Default, Clone, strum::Display, strum::EnumIter, PartialEq, PartialOrd)]
pub enum WebcamResolution {
    #[default]
    #[strum(serialize = "auto", to_string = "auto")]
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

impl WebcamResolution {
    pub fn next(&self) -> Self {
        use strum::IntoEnumIterator;
        let mut iter = WebcamResolution::iter();
        iter.find(|x| x > self).unwrap_or(WebcamResolution::Auto)
    }

    pub fn previous(&self) -> Self {
        use strum::IntoEnumIterator;
        let mut iter = WebcamResolution::iter().rev();
        iter.find(|x| x < self).unwrap_or(WebcamResolution::P4320)
    }
}

impl From<String> for WebcamResolution {
    fn from(s: String) -> Self {
        match s.as_str() {
            "P144" => WebcamResolution::P144,
            "P240" => WebcamResolution::P240,
            "P360" => WebcamResolution::P360,
            "P480" => WebcamResolution::P480,
            "P720" => WebcamResolution::P720,
            "P1080" => WebcamResolution::P1080,
            "P1440" => WebcamResolution::P1440,
            "P2160" => WebcamResolution::P2160,
            "P4320" => WebcamResolution::P4320,
            _ => WebcamResolution::Auto,
        }
    }
}

#[derive(Debug, Default, Clone, strum::Display, strum::EnumIter, PartialEq, PartialOrd, serde::Deserialize)]
pub enum NoiseSuppression {
    #[default]
    #[strum(serialize = "none", to_string = "none")]
    Disabled,
    #[strum(serialize = "deepfilternet", to_string = "deepfilternet")]
    Deepfilternet,
    #[strum(serialize = "rnnoise", to_string = "rnnoise")]
    RNNoise,
    #[strum(serialize = "iris-inta", to_string = "iris-inta")]
    IRISInta,
    #[strum(serialize = "iris-shepherd", to_string = "iris-shepherd")]
    IRISShepherd,
    #[strum(serialize = "krisp-high", to_string = "krisp-high")]
    KrispHigh,
    #[strum(serialize = "krisp-medium", to_string = "krisp-medium")]
    KrispMedium,
    #[strum(serialize = "krisp-low", to_string = "krisp-low")]
    KrispLow,
    #[strum(serialize = "krisp-high-with-bvc", to_string = "krisp-high-with-bvc")]
    KrispHighWithBVC,
    #[strum(serialize = "krisp-medium-with-bvc", to_string = "krisp-medium-with-bvc")]
    KrispMediumWithBVC,
}

impl NoiseSuppression {
    pub fn next(&self) -> Self {
        use strum::IntoEnumIterator;
        let mut iter = NoiseSuppression::iter();
        iter.find(|x| x > self).unwrap_or(NoiseSuppression::Disabled)
    }

    pub fn previous(&self) -> Self {
        use strum::IntoEnumIterator;
        let mut iter = NoiseSuppression::iter().rev();
        iter.find(|x| x < self).unwrap_or(NoiseSuppression::KrispMediumWithBVC)
    }
}

impl From<String> for NoiseSuppression {
    fn from(s: String) -> Self {
        match s.as_str() {
            "deepfilternet" => NoiseSuppression::Deepfilternet,
            "rnnoise" => NoiseSuppression::RNNoise,
            "iris-inta" => NoiseSuppression::IRISInta,
            "iris-shepherd" => NoiseSuppression::IRISShepherd,
            "krisp-high" => NoiseSuppression::KrispHigh,
            "krisp-medium" => NoiseSuppression::KrispMedium,
            "krisp-low" => NoiseSuppression::KrispLow,
            "krisp-high-with-bvc" => NoiseSuppression::KrispHighWithBVC,
            "krisp-medium-with-bvc" => NoiseSuppression::KrispMediumWithBVC,
            _ => NoiseSuppression::Disabled,
        }
    }
}

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

#[cfg(test)]
mod test {
    #[test]
    fn toggle_through_webcam_resolutions() {
        use super::WebcamResolution;
        let mut res = WebcamResolution::Auto;
        res = res.next();
        assert_eq!(res, WebcamResolution::P144);
        res = res.next();
        assert_eq!(res, WebcamResolution::P240);
        res = res.next();
        assert_eq!(res, WebcamResolution::P360);
        res = res.next();
        assert_eq!(res, WebcamResolution::P480);
        res = res.next();
        assert_eq!(res, WebcamResolution::P720);
        res = res.next();
        assert_eq!(res, WebcamResolution::P1080);
        res = res.next();
        assert_eq!(res, WebcamResolution::P1440);
        res = res.next();
        assert_eq!(res, WebcamResolution::P2160);
        res = res.next();
        assert_eq!(res, WebcamResolution::P4320);
        res = res.next();
        assert_eq!(res, WebcamResolution::Auto);

        res = WebcamResolution::P4320;
        res = res.previous();
        assert_eq!(res, WebcamResolution::P2160);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P1440);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P1080);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P720);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P480);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P360);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P240);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P144);
        res = res.previous();
        assert_eq!(res, WebcamResolution::Auto);
        res = res.previous();
        assert_eq!(res, WebcamResolution::P4320);
    }
}
