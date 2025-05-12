pub struct BrowserConfig {
    pub(crate) instance_id: usize,
    pub(crate) url: String,
    pub(crate) cookie: String,
    pub(crate) fake_media: bool,
    pub(crate) fake_video_file: Option<String>,
    #[expect(unused)]
    pub(crate) verbose: bool,
    #[expect(unused)]
    pub(crate) debug: bool,
}

impl From<&super::Config> for BrowserConfig {
    fn from(config: &super::Config) -> Self {
        Self {
            instance_id: 0,
            url: config.url.clone(),
            cookie: config.cookie.clone(),
            fake_media: config.fake_media,
            fake_video_file: config.fake_video_file.clone(),
            verbose: config.verbose,
            debug: config.debug,
        }
    }
}
