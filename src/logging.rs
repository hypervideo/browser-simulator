use crate::config;
use tracing_subscriber::prelude::*;
use tui_logger::TuiLoggerFile;

lazy_static::lazy_static! {
    static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

pub fn log_init() {
    let directory = config::get_data_dir();
    std::fs::create_dir_all(directory.clone()).expect("Failed to create directory");
    let log_path = directory.join(LOG_FILE.clone());

    tui_logger::init_logger(tui_logger::LevelFilter::Trace).expect("Failed to initialize tui logger");
    tui_logger::set_level_for_target("log", tui_logger::LevelFilter::Debug);
    tui_logger::set_log_file(TuiLoggerFile::new(log_path.to_str().unwrap()));

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(tui_logger::TuiTracingSubscriberLayer)
        .try_init()
        .expect("Failed to initialize tracing subscriber");
}
