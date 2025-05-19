use clap::Parser;
use directories::ProjectDirs;
use hyper_video_client_simulator::{
    init_errors,
    media::{
        FakeMediaFileOrUrl,
        FakeMediaFiles,
    },
};
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
    init_errors().expect("Failed to initialize error handling");
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
