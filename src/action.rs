use crate::components::{
    browser_start,
    modal,
};
use serde::{
    Deserialize,
    Serialize,
};
use serde_yml::with::singleton_map_recursive;
use strum::Display;

#[derive(Display, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ClearScreen,
    Error(String),
    Help,

    #[allow(clippy::enum_variant_names)]
    #[serde(with = "singleton_map_recursive")]
    TextModal(modal::TextModalAction),

    #[allow(clippy::enum_variant_names)]
    #[serde(with = "singleton_map_recursive")]
    BrowserStartAction(browser_start::BrowserStartAction),
}

#[cfg(test)]
mod tests {
    use super::*;
    use modal::TextModalAction;

    #[test]
    fn name() {
        let result = serde_yml::to_string(&Action::TextModal(TextModalAction::TextModalSubmit(
            "Testing".to_string(),
        )))
        .unwrap();
        println!("{result}");
        panic!();
    }
}
