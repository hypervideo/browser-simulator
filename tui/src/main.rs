use clap::Parser;
use client_simulator_config::Args;
use client_simulator_tui::{
    init_errors,
    logging,
    App,
};
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    init_errors()?;
    logging::log_init()?;

    App::new(Args::parse())?.run().await
}
