use rnglib::{
    Language,
    RNG,
};

#[derive(Debug, Clone)]
pub struct BrowserConfig {
    pub(crate) name: String,
    pub(crate) fake_media: bool,
    pub(crate) fake_video_file: Option<String>,
    pub(crate) headless: bool,
}

impl BrowserConfig {
    pub fn new(config: &super::Config) -> Self {
        let rng = RNG::from(&Language::Fantasy);
        let name = rng.generate_name_by_count(3);

        Self {
            name,
            fake_media: config.fake_media,
            fake_video_file: config.fake_video_file.clone(),
            headless: config.headless,
        }
    }
}

impl From<&super::Config> for BrowserConfig {
    fn from(config: &super::Config) -> Self {
        Self::new(config)
    }
}

impl From<&super::ParticipantConfig> for BrowserConfig {
    fn from(config: &super::ParticipantConfig) -> Self {
        Self {
            name: config.name.clone(),
            fake_media: config.fake_media,
            fake_video_file: config.fake_video_file.clone(),
            headless: config.headless,
        }
    }
}
