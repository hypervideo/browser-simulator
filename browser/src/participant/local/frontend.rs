use super::super::shared::{
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    ParticipantLaunchSpec,
    ParticipantState,
};
use chromiumoxide::{
    Element,
    Page,
};
use eyre::{
    Context as _,
    Result,
};
use futures::future::BoxFuture;
use tokio::sync::mpsc::UnboundedSender;

/// Local-only context shared by the Chromium session and frontend automation.
#[derive(Debug)]
pub(super) struct FrontendContext {
    pub(super) launch_spec: ParticipantLaunchSpec,
    pub(super) page: Page,
    pub(super) sender: UnboundedSender<ParticipantLogMessage>,
}

impl FrontendContext {
    pub(super) fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    pub(super) fn send_log_message(&self, level: &str, message: impl ToString) {
        if let Err(err) = self
            .sender
            .send(ParticipantLogMessage::new(level, self.participant_name(), message))
        {
            trace!(participant = %self.participant_name(), "Failed to send log message: {err}");
        }
    }

    pub(super) async fn find_element(&self, selector: &str) -> Result<Element> {
        self.page
            .find_element(selector)
            .await
            .context(format!("Could not find the {selector} element"))
    }
}

/// Local DOM automation contract for a concrete Hyper frontend.
pub(super) trait FrontendAutomation: Send {
    fn join(&mut self) -> BoxFuture<'_, Result<()>>;
    fn leave(&mut self) -> BoxFuture<'_, Result<()>>;
    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>>;
    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>>;
}

pub(super) async fn element_state(el: &Element) -> Option<bool> {
    el.attribute("data-test-state")
        .await
        .ok()
        .unwrap_or(None)
        .map(|value| value == "true")
}
