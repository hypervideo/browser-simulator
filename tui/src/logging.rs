use client_simulator_config::get_data_dir;
use eyre::{
    Context as _,
    Result,
};
use tracing_subscriber::prelude::*;
use tui_logger::TuiLoggerFile;

lazy_static::lazy_static! {
    static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

pub fn log_init() -> Result<()> {
    let directory = get_data_dir();
    std::fs::create_dir_all(directory.clone()).context("Failed to create directory")?;
    let log_path = directory.join(LOG_FILE.clone());
    if log_path.exists() {
        std::fs::remove_file(&log_path).context("Failed to remove existing log file")?;
    }

    tui_logger::init_logger(tui_logger::LevelFilter::Trace).context("Failed to initialize tui logger")?;
    tui_logger::set_level_for_target("log", tui_logger::LevelFilter::Debug);
    tui_logger::set_log_file(TuiLoggerFile::new(log_path.to_str().unwrap()));

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(tui_logger::TuiTracingSubscriberLayer)
        .try_init()
        .context("Failed to initialize tracing subscriber")
}
