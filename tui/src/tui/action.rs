use super::components::{
    browser_start,
    participants,
};
use crate::tui::keybindings::Keymap;
use serde::{
    Deserialize,
    Serialize,
};
use strum::Display;
use yaml_serde::with::singleton_map_recursive;

#[derive(Display, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ForceQuit,
    ShutdownComplete,
    ClearScreen,
    Error(String),
    Help,
    UpdateGlobalKeybindings(Keymap),

    Activate(ActivateAction),

    #[allow(clippy::enum_variant_names)]
    #[serde(with = "singleton_map_recursive")]
    #[allow(private_interfaces)]
    BrowserStartAction(browser_start::BrowserStartAction),

    #[allow(clippy::enum_variant_names)]
    #[serde(with = "singleton_map_recursive")]
    #[allow(private_interfaces)]
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
        let result = yaml_serde::to_string(&Action::Activate(ActivateAction::BrowserStart)).unwrap();
        println!("{result}");
        // panic!();
    }
}
