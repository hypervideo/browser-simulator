use std::fmt;
use tracing::Level;

const MAX_TEXT_BYTES: usize = 4 * 1024;
const MAX_BATCH_ENTRIES: usize = 500;
const ELLIPSIS: &str = "...";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::participant) enum BrowserLogSource {
    Console,
    Exception,
    Browser,
}

impl BrowserLogSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Exception => "exception",
            Self::Browser => "browser",
        }
    }
}

impl fmt::Display for BrowserLogSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub(in crate::participant) struct BrowserLogEntry {
    pub participant: String,
    pub source: BrowserLogSource,
    pub level: Level,
    pub text: String,
}

impl BrowserLogEntry {
    pub(in crate::participant) fn new(
        participant: impl Into<String>,
        source: BrowserLogSource,
        level: Level,
        text: impl Into<String>,
    ) -> Self {
        Self {
            participant: participant.into(),
            source,
            level,
            text: truncate_text(text.into()),
        }
    }

    pub(in crate::participant) fn emit(&self) {
        let participant = tracing::info_span!("participant", name = %self.participant);
        let _entered = participant.enter();

        match self.level {
            Level::TRACE => tracing::trace!(target: "browser", source = %self.source, "{}", self.text),
            Level::DEBUG => tracing::debug!(target: "browser", source = %self.source, "{}", self.text),
            Level::INFO => tracing::info!(target: "browser", source = %self.source, "{}", self.text),
            Level::WARN => tracing::warn!(target: "browser", source = %self.source, "{}", self.text),
            Level::ERROR => tracing::error!(target: "browser", source = %self.source, "{}", self.text),
        }
    }
}

pub(in crate::participant) fn console_level(kind: &str) -> Level {
    match kind.to_ascii_lowercase().as_str() {
        "debug" | "trace" | "count" | "countreset" | "timestamp" | "timeend" | "timelog" => Level::DEBUG,
        "warn" | "warning" => Level::WARN,
        "error" | "assert" => Level::ERROR,
        _ => Level::INFO,
    }
}

pub(in crate::participant) fn browser_level(level: &str) -> Level {
    match level.to_ascii_lowercase().as_str() {
        "debug" | "fine" | "finer" | "finest" | "trace" | "verbose" => Level::DEBUG,
        "warn" | "warning" => Level::WARN,
        "error" | "severe" => Level::ERROR,
        _ => Level::INFO,
    }
}

pub(in crate::participant) fn emit_browser_log_batch(participant: &str, entries: Vec<BrowserLogEntry>) {
    let dropped = entries.len().saturating_sub(MAX_BATCH_ENTRIES);
    if dropped > 0 {
        BrowserLogEntry::new(
            participant,
            BrowserLogSource::Browser,
            Level::WARN,
            format!("dropped {dropped} browser log entries"),
        )
        .emit();
    }

    for entry in entries.into_iter().skip(dropped) {
        entry.emit();
    }
}

fn truncate_text(mut text: String) -> String {
    if text.len() <= MAX_TEXT_BYTES {
        return text;
    }

    let mut truncate_at = MAX_TEXT_BYTES - ELLIPSIS.len();
    while !text.is_char_boundary(truncate_at) {
        truncate_at -= 1;
    }
    text.truncate(truncate_at);
    text.push_str(ELLIPSIS);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
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

    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buffer: &[u8]) -> IoResult<usize> {
            self.0.lock().expect("lock output").extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    fn capture_logs(run: impl FnOnce()) -> String {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer_output = Arc::clone(&output);
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || SharedBuffer(Arc::clone(&writer_output)))
            .finish();

        tracing::subscriber::with_default(subscriber, run);

        let output = output.lock().expect("lock output").clone();
        String::from_utf8(output).expect("UTF-8 output")
    }

    #[test]
    fn emits_browser_log_with_participant_source_and_target() {
        let output = capture_logs(|| {
            BrowserLogEntry::new(
                "local-fox-3",
                BrowserLogSource::Console,
                Level::INFO,
                "transport connected",
            )
            .emit();
        });

        assert!(output.contains("INFO participant{name=local-fox-3}: browser:"));
        assert!(output.contains("transport connected"));
        assert!(output.contains("source=console"));
    }

    #[test]
    fn batches_drop_oldest_entries_over_limit() {
        let entries = (0..502)
            .map(|index| {
                BrowserLogEntry::new(
                    "aws-owl-7",
                    BrowserLogSource::Console,
                    Level::INFO,
                    format!("browser-entry-{index:03}"),
                )
            })
            .collect();

        let output = capture_logs(|| emit_browser_log_batch("aws-owl-7", entries));

        assert!(output.contains("dropped 2 browser log entries"));
        assert!(!output.contains("browser-entry-000"));
        assert!(!output.contains("browser-entry-001"));
        assert!(output.contains("browser-entry-002"));
        assert!(output.contains("browser-entry-501"));
        assert_eq!(output.lines().count(), 501);
    }

    #[test]
    fn maps_console_levels() {
        assert_eq!(console_level("log"), Level::INFO);
        assert_eq!(console_level("table"), Level::INFO);
        assert_eq!(console_level("debug"), Level::DEBUG);
        assert_eq!(console_level("timeEnd"), Level::DEBUG);
        assert_eq!(console_level("warning"), Level::WARN);
        assert_eq!(console_level("assert"), Level::ERROR);
    }

    #[test]
    fn maps_browser_and_webdriver_levels() {
        assert_eq!(browser_level("verbose"), Level::DEBUG);
        assert_eq!(browser_level("FINE"), Level::DEBUG);
        assert_eq!(browser_level("INFO"), Level::INFO);
        assert_eq!(browser_level("WARNING"), Level::WARN);
        assert_eq!(browser_level("SEVERE"), Level::ERROR);
    }

    #[test]
    fn truncates_long_text_at_a_utf8_boundary() {
        let entry = BrowserLogEntry::new(
            "participant",
            BrowserLogSource::Browser,
            Level::INFO,
            "🦉".repeat(MAX_TEXT_BYTES),
        );

        assert!(entry.text.len() <= MAX_TEXT_BYTES);
        assert!(entry.text.ends_with(ELLIPSIS));
    }
}
