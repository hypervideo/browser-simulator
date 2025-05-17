#[macro_use]
extern crate tracing;

#[macro_use]
extern crate scopeguard;

mod action;
mod app;
pub mod browser;
mod components;
pub mod config;
mod errors;
mod logging;
mod tui;

pub use app::App;
pub use config::Args;
pub use errors::init as init_errors;
pub use logging::{
    init as init_logging,
    LogCollector,
};
