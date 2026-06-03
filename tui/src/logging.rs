use client_simulator_config::get_data_dir;
use eyre::{
    Context as _,
    Result,
};
use tracing::{
    level_filters::LevelFilter,
    Level,
};
use tracing_subscriber::{
    prelude::*,
    EnvFilter,
};
use tui_logger::TuiLoggerFile;

lazy_static::lazy_static! {
    static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

pub fn log_init(filter: EnvFilter) -> Result<()> {
    let directory = get_data_dir();
    std::fs::create_dir_all(directory.clone()).context("Failed to create directory")?;
    let log_path = directory.join(LOG_FILE.clone());
    if log_path.exists() {
        std::fs::remove_file(&log_path).context("Failed to remove existing log file")?;
    }

    let level_filter = tui_level_filter(filter.max_level_hint().unwrap_or(LevelFilter::INFO));

    tui_logger::init_logger(level_filter).context("Failed to initialize tui logger")?;
    tui_logger::set_level_for_target("log", level_filter);
    tui_logger::set_log_file(TuiLoggerFile::new(log_path.to_str().unwrap()));

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(filter)
        .with(tui_logger::TuiTracingSubscriberLayer)
        .try_init()
        .context("Failed to initialize tracing subscriber")
}

fn tui_level_filter(level_filter: LevelFilter) -> tui_logger::LevelFilter {
    match level_filter.into_level() {
        Some(Level::ERROR) => tui_logger::LevelFilter::Error,
        Some(Level::WARN) => tui_logger::LevelFilter::Warn,
        Some(Level::INFO) => tui_logger::LevelFilter::Info,
        Some(Level::DEBUG) => tui_logger::LevelFilter::Debug,
        Some(Level::TRACE) => tui_logger::LevelFilter::Trace,
        None => tui_logger::LevelFilter::Off,
    }
}
