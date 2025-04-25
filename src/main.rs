use clap::Parser;
use hyper_video_client_simulator::{
    args::Args,
    tui,
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt,
    prelude::*,
    EnvFilter,
    Layer,
};

fn init_logging() -> WorkerGuard {
    color_eyre::install().expect("color_eyre init");

    // Configure file logging
    // Rotate daily, keep logs in the 'logs' directory relative to the executable
    let file_appender = tracing_appender::rolling::daily("logs", "client-simulator.log");
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        // Log to a file
        .with(
            fmt::layer()
                .with_writer(non_blocking_appender)
                .with_ansi(false)
                .with_filter(EnvFilter::from_default_env()),
        )
        .with(tracing_error::ErrorLayer::default())
        .init();

    guard
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let _guard = init_logging();
    let args = Args::parse();
    tui::run(args).await?;
    Ok(())
}
