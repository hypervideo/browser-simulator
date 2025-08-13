use clap::Parser;
use client_simulator_config::media::{
    FakeMediaFileOrUrl,
    FakeMediaFiles,
};
use directories::ProjectDirs;
use std::path::PathBuf;

/// Testing `BrowserFakeMedia`.
/// Example usage:
/// ```bash
/// cargo run --release --example media-conversion -- -i https://share.dev.hyper.video/sp.mp4
/// ```
#[derive(Parser, Debug, Clone)]
pub struct Args {
    /// Audio or video file.
    #[clap(short, long, value_name = "FILE or URL")]
    pub input: FakeMediaFileOrUrl,
}

fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let cache_dir = if let Some(dirs) = ProjectDirs::from("video", "hyper", env!("CARGO_PKG_NAME")) {
        dirs.cache_dir().to_path_buf()
    } else {
        PathBuf::from(".cache")
    };

    let now = std::time::Instant::now();
    let result = FakeMediaFiles::from_file_or_url(args.input, cache_dir).unwrap();
    println!("elapsed ms: {}", now.elapsed().as_millis());

    dbg!(result);
}
