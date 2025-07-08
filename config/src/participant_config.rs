use eyre::{
    OptionExt as _,
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
    pub app_config: super::Config,
}

impl ParticipantConfig {
    pub fn new(config: &super::Config, name: Option<impl ToString>) -> Result<Self> {
        let name = if let Some(name) = name {
            name.to_string()
        } else {
            generate_random_name()
        };
        let url = config.url.clone().ok_or_eyre("No session URL provided")?;
        Ok(Self {
            username: name,
            session_url: url,
            app_config: config.clone(),
        })
    }

    pub fn base_url(&self) -> String {
        self.session_url.origin().unicode_serialization()
    }
}

pub fn generate_random_name() -> String {
    let rng = RNG::from(&Language::Goblin);
    rng.generate_name_by_count(3)
}
