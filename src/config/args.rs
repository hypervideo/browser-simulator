use clap::Parser;

/// Client Simulator TUI
#[derive(Parser, Debug, Clone)]
#[command(author, version = version(), about, long_about = None)]
pub struct Args {
    /// Optional URL to override the stored configuration.
    #[clap(long, value_name = "URL")]
    pub url: Option<String>,

    /// Optional authentication cookie to override the stored configuration.
    #[clap(long, value_name = "COOKIE")]
    pub cookie: Option<String>,

    /// Enable or disable fake WebRTC devices/UI.
    ///   - adds `--use-fake-device-for-media-stream`
    ///   - adds `--use-fake-ui-for-media-stream`
    #[clap(long = "fake-media", action)]
    pub fake_media: Option<bool>,

    /// Optional path passed to `--use-file-for-fake-video-capture`.
    #[clap(long = "fake-video-file", value_name = "FILE")]
    pub fake_video_file: Option<String>,

    /// Enables the displaying of logs in the TUI.
    #[clap(long = "verbose", action)]
    pub verbose: bool,

    /// Enables debug mode.
    ///  - adds `.with_head()` when starting the browser
    #[clap(long = "debug", action)]
    pub debug: bool,
}

mod config_ext {
    use super::*;
    use config::{
        Map,
        Source,
        Value,
    };
    use std::collections::HashMap;

    impl Source for Args {
        fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
            Box::new((*self).clone())
        }

        fn collect(&self) -> Result<Map<String, Value>, config::ConfigError> {
            let mut cache = HashMap::<String, Value>::new();
            if let Some(url) = &self.url {
                cache.insert("url".to_string(), url.clone().into());
            }
            if let Some(cookie) = &self.cookie {
                cache.insert("cookie".to_string(), cookie.clone().into());
            }
            if let Some(fake_media) = &self.fake_media {
                cache.insert("fake_media".to_string(), (*fake_media).into());
            }
            if let Some(fake_video_file) = &self.fake_video_file {
                cache.insert("fake_video_file".to_string(), fake_video_file.clone().into());
            }
            if self.verbose {
                cache.insert("verbose".to_string(), true.into());
            }
            Ok(cache)
        }
    }
}

pub fn version() -> String {
    let author = clap::crate_authors!();
    let config_dir_path = crate::config::get_config_dir().display().to_string();
    let data_dir_path = crate::config::get_data_dir().display().to_string();

    format!(
        "\
Authors: {author}

Config directory: {config_dir_path}
Data directory: {data_dir_path}"
    )
}
