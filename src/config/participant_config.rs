use eyre::{
    Context as _,
    Result,
};
use rnglib::{
    Language,
    RNG,
};
use url::Url;

#[derive(Debug, Clone)]
pub struct ParticipantConfig {
    pub username: String,
    pub session_url: Url,
    pub fake_media: bool,
    pub fake_video_file: Option<String>,
    pub headless: bool,
}

impl ParticipantConfig {
    pub fn new(config: &super::Config) -> Result<Self> {
        let rng = RNG::from(&Language::Fantasy);
        let name = rng.generate_name_by_count(3);
        let url = url::Url::parse(&config.url).context("failed to parse url")?;
        Ok(Self {
            username: name,
            session_url: url,
            fake_media: config.fake_media,
            fake_video_file: config.fake_video_file.clone(),
            headless: config.headless,
        })
    }

    pub fn base_url(&self) -> String {
        self.session_url.origin().unicode_serialization()
    }
}
