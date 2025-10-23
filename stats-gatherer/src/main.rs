//! # Hyper.Video Stats Gatherer - Main Entry Point
//!
//! This tool collects and displays analytics data from Hyper.Video sessions by:
//!
//! 1. Connecting directly to ClickHouse database
//! 2. Querying server-level and space-level metrics
//! 3. Processing audio/video processing metrics
//! 4. Displaying results in beautiful terminal tables
//! 5. Optionally exporting data as JSON

use chrono::Utc;
use clap::Parser;
use client_simulator_stats_gatherer::{
    config::Config,
    Collector,
    Orchestrator,
};
use color_eyre::Result;
use std::time::Duration;
use tracing::info;

#[derive(Parser)]
#[command(name = "stats-gatherer")]
#[command(about = "Hyper.Video Analytics Data Gatherer")]
#[command(version)]
struct Cli {
    /// ClickHouse connection URL (e.g., http://clickhouse:8123)
    #[arg(long, env = "HYPER_CLICKHOUSE_URL")]
    clickhouse_url: String,

    /// ClickHouse username
    #[arg(long, env = "HYPER_CLICKHOUSE_USER", default_value = "default")]
    clickhouse_user: String,

    /// ClickHouse password (optional)
    #[arg(long, env = "HYPER_CLICKHOUSE_PASSWORD")]
    clickhouse_password: Option<String>,

    /// Space URL (provide the URL with SPACE_ID for space specific metrics, or without for global metrics)
    #[arg(long, env = "HYPER_SPACE_URL")]
    space_url: String,

    /// Duration to look back for historical data (e.g., "5m", "1h", "30s")
    #[arg(long, default_value = "5m")]
    duration: String,

    /// Start time for data collection (ISO 8601 format, e.g., "2025-09-30T16:00:00Z")
    /// If not provided, will use current time minus duration
    #[arg(long)]
    start_time: Option<String>,

    /// Output file path (optional, if provided exports as JSON, otherwise displays formatted output)
    #[arg(long)]
    output_file: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("stats_gatherer={log_level},clickhouse=warn"))
        .init();

    color_eyre::install()?;

    info!("Starting Hyper.Video Stats Gatherer");
    info!("ClickHouse URL: {}", cli.clickhouse_url);
    info!("Space URL: {}", cli.space_url);
    info!("Duration: {}", cli.duration);

    // Parse duration
    let duration = parse_duration(&cli.duration)?;
    info!("Parsed duration: {:?}", duration);

    // Parse start time if provided, otherwise use current time minus duration
    let start_time = if let Some(start_time_str) = &cli.start_time {
        chrono::DateTime::parse_from_rfc3339(start_time_str)
            .map_err(|e| eyre::eyre!("Invalid start time '{}': {}", start_time_str, e))?
            .with_timezone(&chrono::Utc)
    } else {
        Utc::now() - chrono::Duration::seconds(duration.as_secs() as i64)
    };
    info!("Collection start time: {}", start_time.format("%Y-%m-%d %H:%M:%S UTC"));

    // Extract server URL from space URL for display purposes
    let server_url = extract_server_url_from_space_url(&cli.space_url)?;
    info!("Extracted server URL from space URL: {}", server_url);

    // Create configuration
    let config = Config::new(
        cli.clickhouse_url,
        cli.clickhouse_user,
        cli.clickhouse_password,
        cli.space_url,
        Duration::from_secs(1), // Not used for interval collection anymore
        cli.output_file,
    )?;

    info!("Parsed server URL: {}", config.server_url);
    if let Some(space_id) = &config.space_id {
        info!("Parsed space ID: {}", space_id);
    } else {
        info!("No space ID found in URL - will collect global metrics");
    }

    // Extract output_file before moving config
    let output_file = config.output_file.clone();

    // Create orchestrator
    let mut orchestrator = Orchestrator::new(config).await?;

    orchestrator.collect(start_time, duration.as_secs() as i64).await?;

    println!("{}", orchestrator.format());

    // Additionally export to JSON if output file is specified using unified interface
    if let Some(output_file) = &output_file {
        let json_string = serde_json::to_string_pretty(&orchestrator.summary())?;
        tokio::fs::write(output_file, json_string).await?;
        info!("Data exported successfully to {}", output_file);
    }

    info!("Data collection completed successfully");
    Ok(())
}

fn parse_duration(duration_str: &str) -> Result<Duration> {
    use humantime::parse_duration;
    parse_duration(duration_str).map_err(|e| eyre::eyre!("Invalid duration '{}': {}", duration_str, e))
}

fn extract_server_url_from_space_url(space_url: &str) -> Result<String> {
    let url = url::Url::parse(space_url).map_err(|e| eyre::eyre!("Invalid space URL '{}': {}", space_url, e))?;

    let server_url = format!(
        "{}://{}",
        url.scheme(),
        url.host_str()
            .ok_or_else(|| eyre::eyre!("No host found in space URL: {}", space_url))?
    );

    Ok(server_url)
}
