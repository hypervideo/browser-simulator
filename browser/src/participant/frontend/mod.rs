//! Driver-agnostic Hyper frontend automation, shared by the Local (chromiumoxide)
//! and Device Farm (WebDriver) backends.

mod driver;

pub(in crate::participant) use driver::{
    decode_test_state,
    BrowserDriver,
    FrontendAutomation,
    FrontendContext,
};
