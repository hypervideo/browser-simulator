use super::components::{
    browser_start,
    participants,
};
use crate::config::Keymap;
use serde::{
    Deserialize,
    Serialize,
};
use serde_yml::with::singleton_map_recursive;
use strum::Display;

#[derive(Display, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Action {
    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ClearScreen,
    Error(String),
    Help,
    UpdateGlobalKeybindings(Keymap),

    Activate(ActivateAction),

    #[allow(clippy::enum_variant_names)]
    #[serde(with = "singleton_map_recursive")]
    BrowserStartAction(browser_start::BrowserStartAction),

    #[allow(clippy::enum_variant_names)]
    #[serde(with = "singleton_map_recursive")]
    ParticipantsAction(participants::ParticipantsAction),

    ParticipantCountChanged(usize),
}

#[derive(Display, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActivateAction {
    BrowserStart,
    Participants,
    Logs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name() {
        let result = serde_yml::to_string(&Action::Activate(ActivateAction::BrowserStart)).unwrap();
        println!("{result}");
        // panic!();
    }
}
