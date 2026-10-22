#[macro_use]
extern crate tracing;

mod app_config;
mod args;
mod browser_config;
mod client_config;
mod cloudflare_config;
mod device_farm_config;
pub mod media;
mod participant_config;

use crate::media::{
    FakeMedia,
    FakeMediaWithDescription,
};
use app_config::AppConfig;
pub use app_config::{
    cache_dir,
    get_config_dir,
    get_data_dir,
};
pub use args::TuiArgs;
pub use browser_config::BrowserConfig;
pub use client_config::{
    NoiseSuppression,
    NoiseSuppressionIter,
    OwnOrDefault,
    ParticipantBackendKind,
    TransportMode,
    TransportModeIter,
    VideoConstraint,
    VideoConstraintIter,
    VideoMaxConcurrentTracks,
    VideoMaxConcurrentTracksPreset,
    VideoMaxConcurrentTracksPresetIter,
};
pub use cloudflare_config::CloudflareConfig;
use color_eyre::Result;
use config::{
    builder::DefaultState,
    ConfigBuilder,
};
pub use device_farm_config::{
    default_device_farm_project_arn,
    default_device_farm_region,
    device_farm_aws_access_key_id,
    device_farm_aws_secret_access_key,
    DeviceFarmConfig,
    DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV,
    DEVICE_FARM_AWS_PROFILE,
    DEVICE_FARM_AWS_REGION_ENV,
    DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV,
    DEVICE_FARM_PROJECT_ARN_ENV,
};
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
    str::FromStr as _,
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
    #[serde(default)]
    pub browser_logs: bool,
    #[serde(default, skip_serializing_if = "ParticipantBackendKind::is_local")]
    pub backend: ParticipantBackendKind,
    #[serde(default, skip_serializing_if = "CloudflareConfig::is_default")]
    pub cloudflare: CloudflareConfig,
    #[serde(default, skip_serializing_if = "DeviceFarmConfig::is_default")]
    pub device_farm: DeviceFarmConfig,
    #[serde(default)]
    pub audio_enabled: bool,
    #[serde(default)]
    pub video_enabled: bool,
    #[serde(default)]
    pub screenshare_enabled: bool,
    #[serde(default)]
    pub auto_gain_control: OwnOrDefault<bool>,
    #[serde(default)]
    pub noise_suppression: NoiseSuppression,
    #[serde(default)]
    pub transport: OwnOrDefault<TransportMode>,
    #[serde(default)]
    pub video_constraint_publish_webcam: OwnOrDefault<VideoConstraint>,
    #[serde(default)]
    pub video_constraint_subscribe: OwnOrDefault<VideoConstraint>,
    #[serde(default)]
    pub video_max_concurrent_tracks: OwnOrDefault<VideoMaxConcurrentTracks>,
    #[serde(default)]
    pub blur: OwnOrDefault<bool>,
}

const DEFAULT_CONFIG: &str = include_str!("default-config.yaml");

impl Default for Config {
    fn default() -> Self {
        let mut config: Self = yaml_serde::from_str(DEFAULT_CONFIG).expect("Failed to parse default config");
        config.device_farm.project_arn = default_device_farm_project_arn();
        config.device_farm.region = default_device_farm_region();
        config
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
        if !self.device_farm.is_default() {
            cache.insert(
                "device_farm".to_string(),
                config::ValueKind::Table(HashMap::from_iter([
                    ("project_arn".to_string(), self.device_farm.project_arn.clone().into()),
                    ("region".to_string(), self.device_farm.region.clone().into()),
                    (
                        "url_expires_seconds".to_string(),
                        self.device_farm.url_expires_seconds.into(),
                    ),
                    (
                        "session_max_duration_ms".to_string(),
                        self.device_farm.session_max_duration_ms.into(),
                    ),
                    ("idle_timeout_ms".to_string(), self.device_farm.idle_timeout_ms.into()),
                    (
                        "health_poll_interval_ms".to_string(),
                        self.device_farm.health_poll_interval_ms.into(),
                    ),
                    ("debug".to_string(), self.device_farm.debug.into()),
                ]))
                .into(),
            );
        }
        if let Some(url) = &self.url {
            cache.insert("url".to_string(), url.to_string().into());
        }
        cache.insert("headless".to_string(), (self.headless).into());
        cache.insert("browser_logs".to_string(), self.browser_logs.into());
        cache.insert("audio_enabled".to_string(), self.audio_enabled.into());
        cache.insert("video_enabled".to_string(), self.video_enabled.into());
        cache.insert("screenshare_enabled".to_string(), self.screenshare_enabled.into());
        cache.insert(
            "auto_gain_control".to_string(),
            self.auto_gain_control.to_string().into(),
        );
        cache.insert(
            "noise_suppression".to_string(),
            self.noise_suppression.to_string().into(),
        );
        cache.insert("transport".to_string(), self.transport.to_string().into());
        cache.insert(
            "video_constraint_publish_webcam".to_string(),
            self.video_constraint_publish_webcam.to_string().into(),
        );
        cache.insert(
            "video_constraint_subscribe".to_string(),
            self.video_constraint_subscribe.to_string().into(),
        );
        cache.insert(
            "video_max_concurrent_tracks".to_string(),
            self.video_max_concurrent_tracks.to_string().into(),
        );
        cache.insert("blur".to_string(), self.blur.to_string().into());
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

        let config_file = config_dir.join("config.yaml");
        builder = builder.add_source(
            config::File::from(config_file.as_path())
                .format(config::FileFormat::Yaml)
                .required(false),
        );

        builder = builder.add_source(args);

        Self::from_builder(builder, &config_file)
    }

    /// Builds the configuration from `builder`, reading a noise suppression that hyper.video doesn't take as `none`.
    ///
    /// [`Config::save`] writes the noise suppression, so `config_file` can name an engine that an older simulator
    /// offered. hyper.video drops stored values that it doesn't take, and the simulator does the same, with a warning,
    /// so that every subcommand still starts. The CLI and `--participant` JSON reject an unknown value instead, because
    /// there it is a typo.
    fn from_builder(builder: ConfigBuilder<DefaultState>, config_file: &Path) -> Result<Self, config::ConfigError> {
        let config = builder.build_cloned()?;
        match config.get_string("noise_suppression") {
            Ok(value) if NoiseSuppression::from_str(&value).is_err() => {
                let fallback = NoiseSuppression::Disabled;
                warn!(
                    "{} names noise suppression {value}, which hyper.video doesn't take; using {fallback}",
                    config_file.display()
                );
                builder
                    .set_override("noise_suppression", fallback.to_string())?
                    .build()?
                    .try_deserialize()
            }
            _ => config.try_deserialize(),
        }
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

    /// A config file's noise suppression loads as `none` when it names an engine that hyper.video no longer offers.
    #[test]
    fn config_file_noise_suppression_loads_old_engine_as_none() {
        let values = [
            ("krisp-high", NoiseSuppression::Disabled),
            ("hyper-voice", NoiseSuppression::HyperVoice),
        ];

        for (raw, expected) in values {
            let builder = config::Config::builder()
                .add_source(Config::default())
                .add_source(config::File::from_str(
                    &format!("noise_suppression: {raw}"),
                    config::FileFormat::Yaml,
                ));

            let config = Config::from_builder(builder, Path::new("config.yaml")).expect("config should load");

            assert_eq!(config.noise_suppression, expected, "{raw}");
        }
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

    #[test]
    fn parses_aws_device_farm_backend_and_nested_device_farm_config() {
        let config: Config = config::Config::builder()
            .add_source(Config::default())
            .add_source(config::File::from_str(
                r#"
backend: aws-device-farm
device_farm:
  project_arn: arn:aws:devicefarm:us-west-2:123456789012:testgrid-project:abc
  region: us-west-2
  url_expires_seconds: 300
  session_max_duration_ms: 1800000
  idle_timeout_ms: 180000
  health_poll_interval_ms: 30000
  debug: true
"#,
                config::FileFormat::Yaml,
            ))
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");

        assert_eq!(config.backend, ParticipantBackendKind::AwsDeviceFarm);
        assert_eq!(
            config.device_farm.project_arn,
            "arn:aws:devicefarm:us-west-2:123456789012:testgrid-project:abc"
        );
        assert_eq!(config.device_farm.region, "us-west-2");
        assert!(config.device_farm.debug);
    }

    #[test]
    fn default_device_farm_config_omits_unused_timeouts() {
        let defaults = include_str!("default-config.yaml");
        let device_farm = defaults
            .split_once("device_farm:\n")
            .and_then(|(_, rest)| rest.split_once("\naudio_enabled:"))
            .map(|(device_farm, _)| device_farm)
            .expect("default config should contain a device_farm section");

        assert!(!device_farm.contains("navigation_timeout_ms"));
        assert!(!device_farm.contains("selector_timeout_ms"));
    }

    #[test]
    fn parses_new_video_constraint_settings() {
        let config: Config = config::Config::builder()
            .add_source(Config::default())
            .add_source(config::File::from_str(
                r#"
video_constraint_publish_webcam: 480p
video_constraint_subscribe: 720p
video_max_concurrent_tracks: 2
"#,
                config::FileFormat::Yaml,
            ))
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");

        assert_eq!(
            config.video_constraint_publish_webcam,
            OwnOrDefault::Own(VideoConstraint::P480)
        );
        assert_eq!(
            config.video_constraint_subscribe,
            OwnOrDefault::Own(VideoConstraint::P720)
        );
        assert_eq!(
            config.video_max_concurrent_tracks,
            OwnOrDefault::Own(VideoMaxConcurrentTracks::Limit(2))
        );
    }

    /// The settings that a participant can leave to hyper.video load an explicit value as its own and `default` as
    /// none, from a config file, from a config passed on as a source, and from a saved config.
    #[test]
    fn own_or_default_settings_load_from_file_source_and_saved_config() {
        let from_file: Config = config::Config::builder()
            .add_source(Config::default())
            .add_source(config::File::from_str(
                r#"
auto_gain_control: true
transport: webrtc
video_constraint_publish_webcam: none
video_constraint_subscribe: default
video_max_concurrent_tracks: unlimited
blur: false
"#,
                config::FileFormat::Yaml,
            ))
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");
        let from_source: Config = config::Config::builder()
            .add_source(from_file.clone())
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");
        let saved = yaml_serde::to_string(&from_file).expect("failed to serialize config");
        let from_saved: Config = config::Config::builder()
            .add_source(Config::default())
            .add_source(config::File::from_str(&saved, config::FileFormat::Yaml))
            .build()
            .expect("failed to build config")
            .try_deserialize()
            .expect("failed to deserialize config");

        for config in [from_file, from_source, from_saved] {
            assert_eq!(config.auto_gain_control, OwnOrDefault::Own(true));
            assert_eq!(config.transport, OwnOrDefault::Own(TransportMode::WebRTC));
            assert_eq!(
                config.video_constraint_publish_webcam,
                OwnOrDefault::Own(VideoConstraint::None)
            );
            assert_eq!(config.video_constraint_subscribe, OwnOrDefault::Default);
            assert_eq!(
                config.video_max_concurrent_tracks,
                OwnOrDefault::Own(VideoMaxConcurrentTracks::Unlimited)
            );
            assert_eq!(config.blur, OwnOrDefault::Own(false));
        }
    }

    /// The default config leaves every setting that a participant can leave to hyper.video to it, and runs no noise
    /// suppression model.
    #[test]
    fn default_config_follows_hyper_video_but_suppresses_no_noise() {
        let config = Config::default();

        assert_eq!(config.auto_gain_control, OwnOrDefault::Default);
        assert_eq!(config.transport, OwnOrDefault::Default);
        assert_eq!(config.video_constraint_publish_webcam, OwnOrDefault::Default);
        assert_eq!(config.video_constraint_subscribe, OwnOrDefault::Default);
        assert_eq!(config.video_max_concurrent_tracks, OwnOrDefault::Default);
        assert_eq!(config.blur, OwnOrDefault::Default);
        assert_eq!(config.noise_suppression, NoiseSuppression::Disabled);
    }

    #[test]
    fn browser_logs_are_off_by_default() {
        assert!(!Config::default().browser_logs);
    }

    #[test]
    fn old_config_without_browser_logs_still_loads() {
        let yaml = include_str!("default-config.yaml").replace("browser_logs: false\n", "");
        let config: Config = yaml_serde::from_str(&yaml).expect("old config should load");

        assert!(!config.browser_logs);
    }
}
