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

    pub(in crate::participant) fn log_message(&self, level: &str, message: impl ToString) {
        ParticipantLogMessage::new(level, self.participant_name(), message).write();
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

#[cfg(test)]
mod tests {
    use super::{
        super::commands::tests::RecordingDriver,
        *,
    };
    use client_simulator_config::{
        Config,
        ParticipantConfig,
    };
    use std::{
        io::{
            Result as IoResult,
            Write,
        },
        sync::{
            Arc,
            Mutex,
        },
    };
    use url::Url;

    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buffer: &[u8]) -> IoResult<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    #[test]
    fn frontend_log_messages_are_written_to_tracing() {
        let context = FrontendContext {
            launch_spec: ParticipantLaunchSpec::from(ParticipantConfig {
                username: "sim-user".to_string(),
                session_url: Url::parse("https://example.com/room").unwrap(),
                app_config: Config::default(),
            }),
            driver: Box::new(RecordingDriver::default()),
        };
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer_output = Arc::clone(&output);
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || SharedBuffer(Arc::clone(&writer_output)))
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            context.log_message("info", "Joined the space");
        });

        let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(output.contains("INFO"));
        assert!(output.contains("Joined the space"));
    }
}
