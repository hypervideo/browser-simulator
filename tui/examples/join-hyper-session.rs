use clap::Parser;
use client_simulator_browser::{
    auth::HyperSessionCookieManger,
    participant::Participant,
};
use client_simulator_config::{
    Config,
    ParticipantConfig,
};
use eyre::Result;
use std::time::Duration;
use url::Url;

#[derive(Parser, Debug, Clone)]
pub struct Args {
    #[clap(long, value_name = "URL")]
    pub url: Url,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    run(Args::parse()).await.unwrap();
}

async fn run(Args { url }: Args) -> Result<()> {
    let (participant, _) = Participant::with_participant_config(
        ParticipantConfig {
            username: "browser-simulator example".to_string(),
            session_url: url.clone(),
            app_config: Config {
                headless: false,
                ..Default::default()
            },
        },
        None,
        HyperSessionCookieManger::new("cookies.json"),
    )
    .expect("Failed to create participant config");

    tokio::signal::ctrl_c().await.expect("Failed to set up signal handler");

    tokio::time::timeout(Duration::from_secs(3), participant.close())
        .await
        .expect("Failed to shutdown");

    Ok(())
}
