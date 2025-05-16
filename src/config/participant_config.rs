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
    pub(crate) name: String,
    pub(crate) url: Url,
    // TODO: combine this and AuthToken
    #[expect(unused)]
    pub(crate) cookie: String,
    pub(crate) fake_media: bool,
    pub(crate) fake_video_file: Option<String>,
    pub(crate) headless: bool,
}

impl ParticipantConfig {
    pub fn new(config: &super::Config) -> Result<Self> {
        let rng = RNG::from(&Language::Fantasy);
        let name = rng.generate_name_by_count(3);

        let url = url::Url::parse(&config.url).context("failed to parse url")?;

        Ok(Self {
            name,
            url,
            cookie: config.cookie.clone(),
            fake_media: config.fake_media,
            fake_video_file: config.fake_video_file.clone(),
            headless: config.headless,
        })
    }
}
