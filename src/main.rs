mod errors;

use clap::{
    Parser,
    Subcommand,
};
use client_simulator_config::TuiArgs;
use client_simulator_tui::start_tui;
use eyre::{
    Context as _,
    OptionExt as _,
};
use tracing_subscriber::{
    fmt,
    prelude::*,
    registry,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Increase verbosity level (can be used multiple times). 1x = info, 2x = debug, 3x = trace.
    #[clap(long = "debug", action = clap::ArgAction::Count)]
    pub debug: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the TUI application
    Tui(TuiArgs),
    /// Connect to the hyper server to get a hyper session cookie
    Cookie(CookieArgs),
}

#[derive(clap::Args, Debug, Clone)]
pub struct CookieArgs {
    /// Base URL of the hyper server
    #[clap(long = "url", value_name = "URL", default_value = "http://localhost:8081")]
    pub base_url: url::Url,

    /// Username for the hyper session
    #[clap(long, value_name = "USERNAME", default_value = "browser-simulator user")]
    pub user: String,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    errors::init()?;

    let CliArgs { command, debug } = CliArgs::parse();

    match command {
        None => {
            let args = TuiArgs {
                debug,
                ..Default::default()
            };
            start_tui(args).await
        }
        Some(Command::Tui(mut args)) => {
            args.debug = debug;
            start_tui(args).await
        }
        Some(Command::Cookie(args)) => run_cookie(args, debug).await,
    }
}

async fn run_cookie(CookieArgs { base_url, user }: CookieArgs, debug: u8) -> eyre::Result<()> {
    if debug > 0 {
        let filter = match debug {
            1 => "info",
            2 => "debug",
            _ => "trace",
        };

        registry()
            .with(
                fmt::layer()
                    .with_span_events(fmt::format::FmtSpan::CLOSE)
                    .with_filter(tracing_subscriber::EnvFilter::builder().parse_lossy(filter)),
            )
            .with(tracing_error::ErrorLayer::default())
            .init();
    }

    let domain = base_url
        .host_str()
        .ok_or_eyre("Base URL must have a valid host")?
        .to_string();
    let config = client_simulator_config::Config::new(Default::default()).context("Failed to create config")?;
    let participants_store = client_simulator_browser::participant::ParticipantStore::new(config.data_dir());
    let cookie = participants_store
        .cookies()
        .give_or_fetch_cookie(base_url, user)
        .await
        .context("Failed to fetch or give cookie")?;
    let cookie = cookie
        .as_browser_cookie_for(&domain)
        .context("Failed to convert cookie for browser")?;
    let json = serde_json::to_string(&cookie).context("Failed to serialize cookie to JSON")?;

    println!("{json}");

    Ok(())
}
