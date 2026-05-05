#[macro_use]
extern crate tracing;

mod app_config;
mod args;
mod browser_config;
mod client_config;
mod cloudflare_config;
pub mod media;
mod participant_config;

use crate::media::{
    FakeMedia,
    FakeMediaWithDescription,
};
use app_config::AppConfig;
pub use app_config::{
    get_config_dir,
    get_data_dir,
};
pub use args::TuiArgs;
pub use browser_config::BrowserConfig;
pub use client_config::{
    NoiseSuppression,
    NoiseSuppressionIter,
    ParticipantBackendKind,
    TransportMode,
    TransportModeIter,
    WebcamResolution,
    WebcamResolutionIter,
};
pub use cloudflare_config::CloudflareConfig;
use color_eyre::Result;
use eyre::Context as _;
pub use participant_config::{
    generate_random_name,
    ParticipantConfig,
};
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
    pub app_config: AppConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<url::Url>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fake_media_selected: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fake_media_sources: Vec<FakeMediaWithDescription>,
    #[serde(default)]
    pub headless: bool,
    #[serde(default, skip_serializing_if = "ParticipantBackendKind::is_local")]
    pub backend: ParticipantBackendKind,
    #[serde(default, skip_serializing_if = "CloudflareConfig::is_default")]
    pub cloudflare: CloudflareConfig,
    #[serde(default)]
    pub audio_enabled: bool,
    #[serde(default)]
    pub video_enabled: bool,
    #[serde(default)]
    pub screenshare_enabled: bool,
    #[serde(default = "default_auto_gain_control")]
    pub auto_gain_control: bool,
    #[serde(default)]
    pub noise_suppression: NoiseSuppression,
    #[serde(default)]
    pub transport: TransportMode,
    #[serde(default)]
    pub resolution: WebcamResolution,
    #[serde(default)]
    pub blur: bool,
}

const DEFAULT_CONFIG: &str = include_str!("default-config.yaml");

const fn default_auto_gain_control() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        yaml_serde::from_str(DEFAULT_CONFIG).expect("Failed to parse default config")
    }
}

impl config::Source for Config {
    fn clone_into_box(&self) -> Box<dyn config::Source + Send + Sync> {
        Box::new((*self).clone())
    }

    fn collect(&self) -> Result<config::Map<String, config::Value>, config::ConfigError> {
        let mut cache = HashMap::<String, config::Value>::new();
        cache.insert("backend".to_string(), self.backend.to_string().into());
        if !self.cloudflare.is_default() {
            cache.insert(
                "cloudflare".to_string(),
                config::ValueKind::Table(HashMap::from_iter([
                    ("base_url".to_string(), self.cloudflare.base_url.to_string().into()),
                    (
                        "request_timeout_seconds".to_string(),
                        self.cloudflare.request_timeout_seconds.into(),
                    ),
                    (
                        "session_timeout_ms".to_string(),
                        self.cloudflare.session_timeout_ms.into(),
                    ),
                    (
                        "navigation_timeout_ms".to_string(),
                        self.cloudflare.navigation_timeout_ms.into(),
                    ),
                    (
                        "selector_timeout_ms".to_string(),
                        self.cloudflare.selector_timeout_ms.into(),
                    ),
                    ("debug".to_string(), self.cloudflare.debug.into()),
                    (
                        "health_poll_interval_ms".to_string(),
                        self.cloudflare.health_poll_interval_ms.into(),
                    ),
                ]))
                .into(),
            );
        }
        if let Some(url) = &self.url {
            cache.insert("url".to_string(), url.to_string().into());
        }
        cache.insert("headless".to_string(), (self.headless).into());
        cache.insert("audio_enabled".to_string(), self.audio_enabled.into());
        cache.insert("video_enabled".to_string(), self.video_enabled.into());
        cache.insert("screenshare_enabled".to_string(), self.screenshare_enabled.into());
        cache.insert("auto_gain_control".to_string(), self.auto_gain_control.into());
        cache.insert(
            "noise_suppression".to_string(),
            self.noise_suppression.to_string().into(),
        );
        cache.insert("transport".to_string(), self.transport.to_string().into());
        cache.insert("resolution".to_string(), self.resolution.to_string().into());
        cache.insert("blur".to_string(), self.blur.into());
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
    pub fn new(args: TuiArgs) -> Result<Self, config::ConfigError> {
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

    pub fn add_custom_fake_media(&mut self, content: String) -> Option<usize> {
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
            clone.url = None;
        }

        std::fs::create_dir_all(&self.app_config.config_dir).context("Failed to create config directory")?;
        let path = self.app_config.config_dir.join("config.yaml");
        let content = yaml_serde::to_string(&clone).context("Failed to serialize config")?;
        std::fs::write(&path, content).wrap_err_with(|| format!("Failed to write config to {:?}", path))
    }

    /// Updates the configuration based on optional command-line arguments.
    /// Saves the configuration if any changes were made.
    ///
    /// # Errors
    /// Returns an error if saving the updated configuration fails.
    #[instrument(level = "debug", skip(self, args))]
    pub fn update_from_args(&mut self, args: &TuiArgs) -> Result<()> {
        let mut changed = false;
        if let Some(url) = &args.url {
            if let Ok(url) = url::Url::parse(url) {
                if self.url.as_ref().is_some_and(|u| u != &url) {
                    info!(old = ?self.url, new = ?url, "Updating URL from args");
                    self.url = Some(url);
                    changed = true;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_old_config_file_without_remote_url_fields() {
        let config: Config = config::Config::builder()
            .add_source(Config::default())
            .add_source(config::File::from_str(
                r#"
url: https://example.com/space/demo
remote_url: 0
remote_url_options:
  - description: old worker
    url: https://remote.example.com
"#,
                config::FileFormat::Yaml,
            ))
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");

        assert_eq!(
            config.url,
            Some(url::Url::parse("https://example.com/space/demo").expect("valid url"))
        );
        assert_eq!(config.backend, ParticipantBackendKind::Local);
    }

    #[test]
    fn parses_cloudflare_backend_and_nested_cloudflare_config() {
        let config: Config = config::Config::builder()
            .add_source(Config::default())
            .add_source(config::File::from_str(
                r#"
backend: cloudflare
cloudflare:
  base_url: https://worker.example.com
  request_timeout_seconds: 15
  session_timeout_ms: 120000
  navigation_timeout_ms: 30000
  selector_timeout_ms: 10000
  debug: true
  health_poll_interval_ms: 2000
"#,
                config::FileFormat::Yaml,
            ))
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");

        assert_eq!(config.backend, ParticipantBackendKind::Cloudflare);
        assert_eq!(
            config.cloudflare.base_url,
            url::Url::parse("https://worker.example.com").expect("valid url")
        );
        assert_eq!(config.cloudflare.request_timeout_seconds, 15);
        assert_eq!(config.cloudflare.session_timeout_ms, 120_000);
        assert_eq!(config.cloudflare.navigation_timeout_ms, 30_000);
        assert_eq!(config.cloudflare.selector_timeout_ms, 10_000);
        assert!(config.cloudflare.debug);
        assert_eq!(config.cloudflare.health_poll_interval_ms, 2_000);
    }
}
