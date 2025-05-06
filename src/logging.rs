use crate::config;
use color_eyre::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt,
    prelude::*,
    EnvFilter,
};

lazy_static::lazy_static! {
    static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

pub fn init() -> Result<()> {
    let directory = config::get_data_dir();
    std::fs::create_dir_all(directory.clone())?;
    let log_path = directory.join(LOG_FILE.clone());
    let log_file = std::fs::File::create(log_path)?;
    let env_filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::TRACE.into())
        .from_env_lossy();
    let file_subscriber = fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(log_file)
        .with_target(false)
        .with_ansi(false)
        .with_filter(env_filter);
    tracing_subscriber::registry()
        .with(file_subscriber)
        .with(ErrorLayer::default())
        .try_init()?;
    Ok(())
}
