use client_simulator_config::TuiArgs;

#[macro_use]
extern crate tracing;

pub mod logging;
mod tui;

pub use tui::Tui;

pub async fn start_tui(args: TuiArgs) -> eyre::Result<()> {
    logging::log_init(args.debug)?;

    tui::App::new(args)?.run().await
}
