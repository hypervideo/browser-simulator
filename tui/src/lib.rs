use client_simulator_config::TuiArgs;
use tracing_subscriber::EnvFilter;

#[macro_use]
extern crate tracing;

pub mod logging;
mod tui;

pub use tui::Tui;

pub async fn start_tui(args: TuiArgs, filter: EnvFilter) -> eyre::Result<()> {
    logging::log_init(filter)?;

    tui::App::new(args)?.run().await
}
