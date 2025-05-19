use clap::Parser;
use color_eyre::Result;
use hyper_video_client_simulator::{
    init_errors,
    logging,
    App,
    Args,
};

#[tokio::main]
async fn main() -> Result<()> {
    init_errors()?;
    logging::log_init();

    App::new(Args::parse())?.run().await
}
