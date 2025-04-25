use color_eyre::{
    eyre::Context,
    Result,
};
use serde::{
    Deserialize,
    Serialize,
};
use std::{
    fs,
    io::{
        self,
        ErrorKind,
    },
    path::PathBuf,
};

const CONFIG_DIR_NAME: &str = "hyper-client-simulator";
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    pub cookie: String,
    pub fake_media: bool,
    pub fake_video_file: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: "https://latest.dev.hyper.video".to_string(),
            cookie: String::new(),
            fake_media: true,
            fake_video_file: None,
        }
    }
}

impl Config {
    fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "Config directory not found"))?
            .join(CONFIG_DIR_NAME);

        fs::create_dir_all(&config_dir)
            .wrap_err_with(|| format!("Failed to create config directory at {:?}", config_dir))?;

        Ok(config_dir.join(CONFIG_FILE_NAME))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        match fs::read_to_string(&path) {
            Ok(content) => {
                serde_json::from_str(&content).wrap_err_with(|| format!("Failed to deserialize config from {:?}", path))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                info!("Config file not found at {:?}, creating default.", path);
                let config = Config::default();
                config.save().wrap_err("Failed to save default config")?;
                Ok(config)
            }
            Err(e) => Err(e).wrap_err_with(|| format!("Failed to read config file from {:?}", path)),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let content = serde_json::to_string_pretty(self).wrap_err("Failed to serialize config")?;
        fs::write(&path, content).wrap_err_with(|| format!("Failed to write config to {:?}", path))
    }

    /// Updates the configuration based on optional command-line arguments.
    /// Saves the configuration if any changes were made.
    ///
    /// # Errors
    /// Returns an error if saving the updated configuration fails.
    #[instrument(level = "debug", skip(self, args))]
    pub fn update_from_args(&mut self, args: &crate::args::Args) -> Result<()> {
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
