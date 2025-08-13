use crate::{
    auth::{
        BorrowedCookie,
        HyperSessionCookie,
        HyperSessionCookieManger,
    },
    participant::{
        messages::ParticipantLogMessage,
        ParticipantState,
    },
};
use base64::{
    prelude::BASE64_STANDARD,
    Engine,
};
use client_simulator_config::{
    generate_random_name,
    media::{
        FakeMedia,
        FakeMediaWithDescription,
    },
    Config,
    NoiseSuppression,
    TransportMode,
    WebcamResolution,
};
use eyre::{
    eyre,
    Context as _,
    Report,
    Result,
};
use std::fmt;
use strum::Display;
use url::Url;

#[derive(Debug, Default, Display, Clone, serde::Serialize, serde::Deserialize)]
pub enum FakeMediaQuery {
    #[default]
    None,
    Builtin,
    Url(Url),
}

impl From<FakeMedia> for FakeMediaQuery {
    fn from(source: FakeMedia) -> Self {
        Self::from(&source)
    }
}

impl From<&FakeMedia> for FakeMediaQuery {
    fn from(source: &FakeMedia) -> Self {
        match source {
            FakeMedia::None => Self::None,
            FakeMedia::Builtin => Self::Builtin,
            FakeMedia::FileOrUrl(file_or_url) => {
                if file_or_url.starts_with("http") {
                    Url::parse(file_or_url).map(FakeMediaQuery::Url).unwrap_or_default()
                } else {
                    FakeMediaQuery::Builtin
                }
            }
        }
    }
}

impl From<&FakeMediaWithDescription> for FakeMediaQuery {
    fn from(source: &FakeMediaWithDescription) -> Self {
        Self::from(source.fake_media())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantConfigQuery {
    pub username: String,
    pub remote_url: Url,
    pub session_url: Url,
    pub base_url: String,
    pub cookie: Option<HyperSessionCookie>,
    pub fake_media: Option<FakeMediaQuery>,
    pub audio_enabled: bool,
    pub video_enabled: bool,
    pub headless: bool,
    pub screenshare_enabled: bool,
    pub noise_suppression: NoiseSuppression,
    pub transport: TransportMode,
    pub resolution: WebcamResolution,
    pub blur: bool,
}

impl ParticipantConfigQuery {
    pub fn new(config: &Config, cookie: Option<&BorrowedCookie>) -> Result<Self> {
        let username = cookie
            .map(|c| c.username().to_string())
            .unwrap_or_else(generate_random_name);

        let fake_media = config
            .fake_media_selected
            .map(|index| config.fake_media_sources.get(index).map(FakeMediaQuery::from))
            .unwrap_or_default();

        let session_url = config
            .url
            .clone()
            .ok_or_else(|| eyre!("Session URL is required for remote participant"))?;

        let base_url = session_url.origin().unicode_serialization();

        let remote_url_index = config
            .remote_url
            .ok_or_else(|| eyre!("Remote URL is required for remote participant"))?;

        let remote_url = config
            .remote_url_options
            .get(remote_url_index)
            .ok_or_else(|| eyre!("Remote URL is required for remote participant"))?
            .url()
            .clone();

        Ok(Self {
            username,
            remote_url,
            session_url,
            base_url,
            cookie: cookie.map(|c| c.cookie.clone()),
            fake_media,
            audio_enabled: config.audio_enabled,
            video_enabled: config.video_enabled,
            headless: config.headless,
            screenshare_enabled: config.screenshare_enabled,
            noise_suppression: config.noise_suppression,
            transport: config.transport,
            resolution: config.resolution,
            blur: config.blur,
        })
    }

    pub async fn ensure_cookie(&mut self, cookie_manager: HyperSessionCookieManger) -> Result<Option<BorrowedCookie>> {
        if self.cookie.is_none() {
            let base_url = Url::parse(&self.base_url).context("Failed to parse base URL")?;
            let cookie = cookie_manager.fetch_new_cookie(base_url, &self.username).await?;
            self.cookie = Some(cookie.cookie.clone());

            return Ok(Some(cookie));
        }

        Ok(None)
    }

    pub fn into_config_and_cookie(
        self,
        app_config: &Config,
        cookie_manager: HyperSessionCookieManger,
    ) -> (Config, Option<BorrowedCookie>) {
        let borrowed_cookie = self
            .cookie
            .map(|cookie| BorrowedCookie::new(&self.base_url, cookie, cookie_manager));

        let mut config = Config {
            app_config: app_config.app_config.clone(),
            url: Some(self.session_url.clone()),
            fake_media_selected: Some(0), // Default to the first fake media source
            fake_media_sources: app_config.fake_media_sources.clone(),
            audio_enabled: self.audio_enabled,
            video_enabled: self.video_enabled,
            headless: self.headless,
            screenshare_enabled: self.screenshare_enabled,
            noise_suppression: self.noise_suppression,
            transport: self.transport,
            resolution: self.resolution,
            blur: self.blur,
            remote_url: None,
            remote_url_options: vec![],
        };

        config.fake_media_selected = match self.fake_media {
            Some(FakeMediaQuery::None) => None,
            Some(FakeMediaQuery::Builtin) => Some(1), // Builtin media is the first source
            Some(FakeMediaQuery::Url(url)) => config.add_custom_fake_media(url.to_string()),
            None => Some(0), // Default to the first source if no fake media is specified
        };

        (config, borrowed_cookie)
    }

    pub fn into_url(&self) -> Result<Url> {
        let json = serde_json::to_string(&self)?;
        let base64 = BASE64_STANDARD.encode(json.as_bytes());
        let mut url = self.remote_url.clone();
        url.query_pairs_mut().append_pair("payload", &base64);
        Ok(url)
    }
}

impl TryFrom<String> for ParticipantConfigQuery {
    type Error = Report;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let json = BASE64_STANDARD.decode(value)?;
        let config = serde_json::from_slice::<Self>(json.as_slice())?;
        Ok(config)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantResponseMessage {
    pub state: ParticipantState,
    pub log: Option<ParticipantLogMessage>,
}

impl ParticipantResponseMessage {
    pub fn new(state: ParticipantState, log: ParticipantLogMessage) -> Self {
        Self { state, log: Some(log) }
    }

    pub fn from_state(state: ParticipantState) -> Self {
        Self { state, log: None }
    }

    pub fn from_log(log: ParticipantLogMessage) -> Self {
        Self {
            state: ParticipantState::default(),
            log: Some(log),
        }
    }
}

impl fmt::Display for ParticipantResponseMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
        )
    }
}
