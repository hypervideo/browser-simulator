use super::super::shared::{
    messages::{
        ParticipantLogMessage,
        ParticipantMessage,
    },
    ParticipantLaunchSpec,
    ParticipantState,
};
use eyre::Result;
use futures::future::BoxFuture;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// Driver-agnostic browser operations used by the Hyper frontend automation.
///
/// The `eval` contract is uniform across drivers: `js_body` is a JavaScript
/// statement list; the optional `arg` is available as `arguments[0]`; use
/// `return` to produce a value. Implementations adapt this to their engine
/// (chromiumoxide wraps it as `function() { <js_body> }` invoked with `arg`;
/// WebDriver passes `js_body`/`arg` straight to `execute`).
pub(in crate::participant) trait BrowserDriver: Send + Sync {
    fn goto(&self, url: &str) -> BoxFuture<'_, Result<()>>;
    /// True if at least one element matches `selector` right now.
    fn exists(&self, selector: &str) -> BoxFuture<'_, Result<bool>>;
    /// Poll until `selector` exists or `timeout` elapses.
    fn wait_for(&self, selector: &str, timeout: Duration) -> BoxFuture<'_, Result<()>>;
    fn click(&self, selector: &str) -> BoxFuture<'_, Result<()>>;
    /// Focus the element, clear its value, then type `text`.
    fn fill(&self, selector: &str, text: &str) -> BoxFuture<'_, Result<()>>;
    /// Read an attribute of the first element matching `selector`.
    /// `Ok(None)` if the element exists but the attribute is absent.
    fn attribute(&self, selector: &str, name: &str) -> BoxFuture<'_, Result<Option<String>>>;
    fn eval(&self, js_body: &str, arg: Option<serde_json::Value>) -> BoxFuture<'_, Result<serde_json::Value>>;
    /// Set a cookie for `domain`. Drivers that require being on-origin first
    /// (WebDriver) must navigate to the origin before setting it.
    fn set_cookie(&self, domain: &str, name: &str, value: &str) -> BoxFuture<'_, Result<()>>;
}

/// Context shared by every frontend automation, parameterised over the driver.
pub(in crate::participant) struct FrontendContext {
    pub(in crate::participant) launch_spec: ParticipantLaunchSpec,
    pub(in crate::participant) driver: Box<dyn BrowserDriver>,
    pub(in crate::participant) sender: UnboundedSender<ParticipantLogMessage>,
}

impl std::fmt::Debug for FrontendContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrontendContext")
            .field("launch_spec", &self.launch_spec)
            .finish_non_exhaustive()
    }
}

impl FrontendContext {
    pub(in crate::participant) fn participant_name(&self) -> &str {
        &self.launch_spec.username
    }

    pub(in crate::participant) fn send_log_message(&self, level: &str, message: impl ToString) {
        if let Err(err) = self
            .sender
            .send(ParticipantLogMessage::new(level, self.participant_name(), message))
        {
            trace!(participant = %self.participant_name(), "Failed to send log message: {err}");
        }
    }
}

/// The DOM automation contract for a concrete Hyper frontend (HyperCore/HyperLite).
pub(in crate::participant) trait FrontendAutomation: Send {
    fn join(&mut self) -> BoxFuture<'_, Result<()>>;
    fn leave(&mut self) -> BoxFuture<'_, Result<()>>;
    fn handle_command(&mut self, message: ParticipantMessage) -> BoxFuture<'_, Result<()>>;
    fn refresh_state(&mut self) -> BoxFuture<'_, Result<ParticipantState>>;
}

/// Decode the legacy `data-test-state="true"|"false"` attribute.
pub(in crate::participant) fn decode_test_state(value: Option<String>) -> Option<bool> {
    value.map(|value| value == "true")
}
