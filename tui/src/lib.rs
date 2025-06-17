#[macro_use]
extern crate tracing;

mod errors;
pub mod logging;
mod tui;

pub use errors::init as init_errors;
pub use tui::App;
