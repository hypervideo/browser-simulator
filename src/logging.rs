use crate::config;
use color_eyre::Result;
use std::{
    fmt,
    sync::{
        Arc,
        Mutex,
    },
};
use tracing::{
    Event,
    Subscriber,
};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::layer,
    layer::Context,
    prelude::*,
    EnvFilter,
    Layer,
};

struct MessageVisitor<'a> {
    buffer: &'a mut String,
}

impl tracing::field::Visit for MessageVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.buffer.push_str(&format!("{:?}", value));
        } else {
            self.buffer.push_str(&format!(" {}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.buffer.push_str(value);
        } else {
            self.buffer.push_str(&format!(" {}={}", field.name(), value));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.buffer.push_str(&format!(" {}={}", field.name(), value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.buffer.push_str(&format!(" {}={}", field.name(), value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.buffer.push_str(&format!(" {}={}", field.name(), value));
    }
}

#[derive(Clone)]
pub struct LogCollector {
    logs: Arc<Mutex<Vec<String>>>,
}

impl PartialEq for LogCollector {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.logs, &other.logs)
    }
}

impl fmt::Debug for LogCollector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let logs = self.logs.lock().unwrap();
        f.debug_struct("LogCollector").field("log_count", &logs.len()).finish()
    }
}

impl LogCollector {
    pub fn new() -> Self {
        LogCollector {
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn get_logs(&self) -> Vec<String> {
        self.logs.lock().unwrap().clone()
    }
}

impl Default for LogCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Subscriber> Layer<S> for LogCollector {
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        let mut buffer = String::new();
        let metadata = event.metadata();
        buffer.push_str(&format!("[{}] ", metadata.level()));
        let target = metadata.target();
        buffer.push_str(&format!("{}: ", target));

        let mut visitor = MessageVisitor { buffer: &mut buffer };
        event.record(&mut visitor);

        let mut logs = self.logs.lock().unwrap();
        logs.push(buffer);
    }
}

lazy_static::lazy_static! {
    static ref LOG_FILE: String = format!("{}.log", env!("CARGO_PKG_NAME"));
}

pub fn init() -> Result<LogCollector> {
    let directory = config::get_data_dir();
    std::fs::create_dir_all(directory.clone())?;
    let log_path = directory.join(LOG_FILE.clone());
    let log_file = std::fs::File::create(log_path)?;
    let env_filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::TRACE.into())
        .from_env_lossy();
    let collector_filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::TRACE.into())
        .from_env_lossy();
    let log_collector = LogCollector::new();
    let file_subscriber = layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(log_file)
        .with_target(false)
        .with_ansi(false)
        .with_filter(env_filter);
    tracing_subscriber::registry()
        .with(file_subscriber)
        .with(ErrorLayer::default())
        .with(log_collector.clone().with_filter(collector_filter))
        .try_init()?;
    Ok(log_collector)
}
