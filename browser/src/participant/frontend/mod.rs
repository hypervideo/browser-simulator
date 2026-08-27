//! Driver-agnostic Hyper frontend automation, shared by the Local (chromiumoxide)
//! and Device Farm (WebDriver) backends.

mod builder;
mod commands;
mod core;
mod driver;
mod lite;
mod selectors;

pub(in crate::participant) use builder::{
    FrontendAuth,
    FrontendKindBuilder,
};
pub(in crate::participant) use driver::{
    first_party_credentials_init_script,
    BrowserDriver,
    FrontendAutomation,
    FrontendContext,
};
