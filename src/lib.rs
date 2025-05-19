#[macro_use]
extern crate tracing;

pub mod browser;
pub mod config;
mod errors;
pub mod logging;
mod tui;

pub use config::Args;
pub use errors::init as init_errors;
pub use tui::App;
