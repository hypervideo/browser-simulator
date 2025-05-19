use clap::Parser;
use eyre::Result;
use hyper_video_client_simulator::{
    browser::participant::Participant,
    config::ParticipantConfig,
    init_errors,
};
use std::time::Duration;
use url::Url;

#[derive(Parser, Debug, Clone)]
pub struct Args {
    #[clap(long, value_name = "URL")]
    pub url: Url,
}

#[tokio::main]
async fn main() {
    init_errors().expect("Failed to initialize error handling");
    tracing_subscriber::fmt::init();

    run(Args::parse()).await.unwrap();
}

async fn run(Args { url }: Args) -> Result<()> {
    let participant = Participant::with_participant_config(ParticipantConfig {
        username: "browser-simulator example".to_string(),
        session_url: url.clone(),
        fake_media: true,
        fake_video_file: None,
        headless: false,
    })
    .expect("Failed to create participant config");

    tokio::signal::ctrl_c().await.expect("Failed to set up signal handler");

    tokio::time::timeout(Duration::from_secs(3), participant.close())
        .await
        .expect("Failed to shutdown");

    Ok(())
}
