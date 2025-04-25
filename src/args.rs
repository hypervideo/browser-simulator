use clap::Parser;

/// Client Simulator TUI
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Optional URL to override the stored configuration.
    #[clap(long)]
    pub url: Option<String>,

    /// Optional authentication cookie to override the stored configuration.
    #[clap(long)]
    pub cookie: Option<String>,

    /// Enable or disable fake WebRTC devices/UI.
    ///   - adds `--use-fake-device-for-media-stream`
    ///   - adds `--use-fake-ui-for-media-stream`
    #[clap(long = "fake-media")]
    pub fake_media: Option<bool>,

    /// Optional path passed to `--use-file-for-fake-video-capture`.
    #[clap(long = "fake-video-file")]
    pub fake_video_file: Option<String>,
}
