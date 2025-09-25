use clap::Parser;
use client_simulator_orchestrator::{
    parse_config,
    run,
};
use color_eyre::Result;
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Layer,
};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct Args {
    /// Path to orchestrator config file (yaml)
    #[arg(long)]
    config: std::path::PathBuf,
}

fn init_logging() {
    color_eyre::install().expect("color_eyre init");
    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(EnvFilter::from_default_env()))
        .with(tracing_error::ErrorLayer::default())
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let args = Args::parse();
    let cfg = parse_config(&args.config)?;
    cfg.validate()?;
    run(cfg).await
}
