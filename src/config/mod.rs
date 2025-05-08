mod app_config;
mod args;
mod keybindings;

use app_config::AppConfig;
pub(crate) use app_config::{
    get_config_dir,
    get_data_dir,
};
pub use args::Args;
use color_eyre::Result;
use eyre::Context as _;
use keybindings::KeyBindings;
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten, skip_serializing)]
    app_config: AppConfig,
    #[serde(skip)]
    pub(crate) keybindings: KeyBindings,
    #[serde(default)]
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) cookie: String,
    #[serde(default)]
    pub(crate) fake_media: bool,
    #[serde(default)]
    pub(crate) fake_video_file: Option<String>,
    #[serde(default)]
    pub(crate) verbose: bool,
}

impl Config {
    pub fn new(args: Args) -> Result<Self, config::ConfigError> {
        let data_dir = get_data_dir();
        let config_dir = get_config_dir();
        let mut builder = config::Config::builder()
            .set_default("data_dir", data_dir.to_str().unwrap())?
            .set_default("config_dir", config_dir.to_str().unwrap())?;

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

    pub fn save(&self) -> Result<()> {
        let path = self.app_config.config_dir.join("config.yaml");
        let content = serde_yml::to_string(self).context("Failed to serialize config")?;
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
        if let Some(cookie) = &args.cookie {
            // Avoid logging the full cookie
            if self.cookie != *cookie {
                info!(old_len = %self.cookie.len(), new_len = %cookie.len(), "Updating cookie from args");
                self.cookie = cookie.clone();
                changed = true;
            }
        }
        if let Some(fake) = args.fake_media {
            if self.fake_media != fake {
                info!(old = %self.fake_media, new = %fake, "Updating fake_media from args");
                self.fake_media = fake;
                changed = true;
            }
        }
        if let Some(path) = &args.fake_video_file {
            if self.fake_video_file.as_ref() != Some(path) {
                info!(old = ?self.fake_video_file, new = %path, "Updating fake_video_file from args");
                self.fake_video_file = Some(path.clone());
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
