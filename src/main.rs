use clap::Parser;
use color_eyre::Result;
use hyper_video_client_simulator::{
    init_errors,
    init_logging,
    App,
    Args,
};

#[tokio::main]
async fn main() -> Result<()> {
    init_errors()?;
    let log_collector = init_logging()?;
    App::new(Args::parse(), log_collector)?.run().await
}
