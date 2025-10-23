use chrono::{
    DateTime,
    Utc,
};
use color_eyre::Result;
use std::{
    future::Future,
    pin::Pin,
};

/// Trait for collecting and formatting data
pub trait Collector {
    /// Collect data for the specified time range
    fn collect(
        &mut self,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    /// Format data for display
    fn format(&self) -> String;

    /// Get data summary as JSON
    fn summary(&self) -> serde_json::Value;

    /// Get the name of this collector
    fn name(&self) -> &'static str;
}
