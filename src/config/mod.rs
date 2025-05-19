mod app_config;
mod args;
mod browser_config;
mod keybindings;
mod participant_config;

use crate::media::{
    FakeMedia,
    FakeMediaWithDescription,
};
use app_config::AppConfig;
pub(crate) use app_config::{
    get_config_dir,
    get_data_dir,
};
pub use args::Args;
pub use browser_config::BrowserConfig;
use color_eyre::Result;
use eyre::Context as _;
pub(crate) use keybindings::{
    KeyBindings,
    Keymap,
};
pub use participant_config::ParticipantConfig;
use serde::{
    Deserialize,
    Serialize,
};
use std::{
    collections::HashMap,
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten, skip_serializing)]
    app_config: AppConfig,
    #[serde(skip)]
    pub(crate) keybindings: KeyBindings,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fake_media_selected: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) fake_media_sources: Vec<FakeMediaWithDescription>,
    #[serde(default)]
    pub(crate) headless: bool,
}

const DEFAULT_CONFIG: &str = include_str!("default-config.yaml");

impl Default for Config {
    fn default() -> Self {
        serde_yml::from_str(DEFAULT_CONFIG).expect("Failed to parse default config")
    }
}

impl config::Source for Config {
    fn clone_into_box(&self) -> Box<dyn config::Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<config::Map<String, config::Value>, config::ConfigError> {
        let mut cache = HashMap::<String, config::Value>::new();
        if !self.url.is_empty() {
            cache.insert("url".to_string(), self.url.clone().into());
        }
        cache.insert("headless".to_string(), (self.headless).into());
        if let Some(value) = self.fake_media_selected {
            cache.insert("fake_media_selected".to_string(), (value as u64).into());
        }
        if !self.fake_media_sources.is_empty() {
            cache.insert(
                "fake_media_sources".to_string(),
                self.fake_media_sources
                    .iter()
                    .map(|ea| {
                        config::ValueKind::Table(HashMap::from_iter([
                            ("description".to_string(), ea.description().to_string().into()),
                            ("fake_media".to_string(), ea.fake_media().to_string().into()),
                        ]))
                    })
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        Ok(cache)
    }
}

impl Config {
    pub fn new(args: Args) -> Result<Self, config::ConfigError> {
        let data_dir = get_data_dir();
        let config_dir = get_config_dir();
        let mut builder = config::Config::builder()
            .set_default("data_dir", data_dir.to_str().unwrap())?
            .set_default("config_dir", config_dir.to_str().unwrap())?;

        builder = builder.add_source(Config::default());

        let config_files = [("config.yaml", config::FileFormat::Yaml)];

        for (file, format) in &config_files {
            let source = config::File::from(config_dir.join(file))
                .format(*format)
                .required(false);
            builder = builder.add_source(source);
        }

        builder = builder.add_source(args);

        let cfg: Self = builder.build()?.try_deserialize()?;

        Ok(cfg)
    }

    pub fn fake_media_with_description(&self) -> FakeMediaWithDescription {
        match (self.fake_media_selected, &self.fake_media_sources) {
            (Some(selected), sources) if selected < sources.len() => sources[selected].clone(),
            _ => FakeMediaWithDescription::default(),
        }
    }

    pub fn fake_media(&self) -> FakeMedia {
        self.fake_media_with_description().fake_media().clone()
    }

    pub(crate) fn add_custom_fake_media(&mut self, content: String) -> Option<usize> {
        let media = if content.trim().is_empty() {
            return None;
        } else {
            FakeMediaWithDescription::new(FakeMedia::FileOrUrl(content.clone()), Some(content))
        };
        let fake_media_sources = &mut self.fake_media_sources;
        if fake_media_sources.len() >= 2 {
            fake_media_sources.insert(2, media);
            Some(2)
        } else {
            fake_media_sources.push(media);
            Some(fake_media_sources.len() - 1)
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.app_config.data_dir
    }

    pub fn save(&self) -> Result<()> {
        // Only save the parts that have changed from the default.
        let default = Self::default();
        let mut clone = self.clone();

        if self.fake_media_selected == default.fake_media_selected {
            clone.fake_media_selected = None;
        }
        if self.fake_media_sources == default.fake_media_sources {
            clone.fake_media_sources = Vec::new();
        }
        if self.url == default.url {
            clone.url = String::new();
        }

        std::fs::create_dir_all(&self.app_config.config_dir).context("Failed to create config directory")?;
        let path = self.app_config.config_dir.join("config.yaml");
        let content = serde_yml::to_string(&clone).context("Failed to serialize config")?;
        std::fs::write(&path, content).wrap_err_with(|| format!("Failed to write config to {:?}", path))
    }

    /// Updates the configuration based on optional command-line arguments.
    /// Saves the configuration if any changes were made.
    ///
    /// # Errors
    /// Returns an error if saving the updated configuration fails.
    #[instrument(level = "debug", skip(self, args))]
    pub fn update_from_args(&mut self, args: &Args) -> Result<()> {
        let mut changed = false;
        if let Some(url) = &args.url {
            if self.url != *url {
                info!(old = %self.url, new = %url, "Updating URL from args");
                self.url = url.clone();
                changed = true;
            }
        }

        if let Some(headless) = args.headless {
            if self.headless != headless {
                info!(old = %self.headless, new = %headless, "Updating headless from args");
                self.headless = headless;
                changed = true;
            }
        }

        if changed {
            debug!("Configuration updated from command-line arguments, saving...");
            self.save()?;
        } else {
            debug!("No configuration changes from command-line arguments.");
        }
        Ok(())
    }
}
