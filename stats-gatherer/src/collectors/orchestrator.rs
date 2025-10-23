use crate::{
    collectors::{
        Collector,
        ParticipantCollector,
        ServerCollector,
        SpaceCollector,
    },
    config::Config,
    metrics::*,
};
use chrono::{
    DateTime,
    Utc,
};
use clickhouse::Client;
use eyre::Result;
use reqwest::Client as HttpClient;
use serde_json;
use std::{
    future::Future,
    pin::Pin,
};

/// Orchestrates all collectors and manages the overall data collection flow
pub struct Orchestrator {
    server_collector: ServerCollector,
    space_collector: Option<SpaceCollector>,
    participant_collector: Option<ParticipantCollector>,
    metrics: Option<CollectedData>,
}

impl Orchestrator {
    /// Create a new orchestrator with all available collectors
    pub async fn new(config: Config) -> Result<Self> {
        // Create shared clients once
        let mut clickhouse_client = Client::default()
            .with_url(config.clickhouse_url.clone())
            .with_user(config.clickhouse_user.clone());

        if let Some(password) = &config.clickhouse_password {
            clickhouse_client = clickhouse_client.with_password(password.clone());
        }

        let http_client = HttpClient::new();

        let server_collector =
            ServerCollector::new(config.clone(), clickhouse_client.clone(), http_client.clone()).await?;

        let space_collector = if config.space_id.is_some() {
            let collector = SpaceCollector::new(config.clone(), clickhouse_client.clone()).await?;
            Some(collector)
        } else {
            None
        };

        let participant_collector = if config.space_id.is_some() {
            Some(ParticipantCollector::new(config.clone(), clickhouse_client.clone()).await?)
        } else {
            None
        };

        Ok(Self {
            server_collector,
            space_collector,
            participant_collector,
            metrics: None,
        })
    }
}

impl Collector for Orchestrator {
    fn collect(
        &mut self,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut collected_data = CollectedData::new(start_time);
            collected_data.collection_duration_seconds = duration_seconds as f64;

            // Collect server-level data
            self.server_collector.collect(start_time, duration_seconds).await?;

            // Collect space-level data if space ID is available
            if let Some(ref mut space_collector) = self.space_collector {
                space_collector.collect(start_time, duration_seconds).await?;
            }

            // Collect participant-level data if space ID is available
            if let Some(ref mut participant_collector) = self.participant_collector {
                participant_collector.collect(start_time, duration_seconds).await?;
            }

            collected_data.finalize();

            self.metrics = Some(collected_data);
            Ok(())
        })
    }

    fn format(&self) -> String {
        let metrics = match &self.metrics {
            Some(m) => m,
            None => panic!("No metrics collected yet. Call collect() first."),
        };

        let mut report = String::new();

        report.push_str(&format!("\n{}\n", "=".repeat(80)));
        report.push_str(&format!("{:^80}\n", "🚀 HYPER.VIDEO ANALYTICS REPORT"));
        report.push_str(&format!("{}\n", "=".repeat(80)));

        // Collection summary
        report.push_str(&format!(
            "\n📊 Collection Summary:\n\
            • Collection Time: {}\n\
            • Duration: {:.1} seconds\n",
            metrics.collection_start.format("%Y-%m-%d %H:%M:%S UTC"),
            metrics.collection_duration_seconds
        ));

        // Server data report
        report.push_str(&self.server_collector.format().to_string());

        // Space data report
        if let Some(ref space_collector) = self.space_collector {
            report.push_str(&space_collector.format().to_string());
        }

        // Participant data report
        if let Some(ref participant_collector) = self.participant_collector {
            report.push_str(&participant_collector.format().to_string());
        }

        report.push_str(&format!("\n{}\n", "=".repeat(80)));
        report.push_str(&format!("{:^80}\n", "✅ END OF REPORT"));
        report.push_str(&format!("{}\n", "=".repeat(80)));

        report
    }

    fn summary(&self) -> serde_json::Value {
        let metrics = match &self.metrics {
            Some(m) => m,
            None => panic!("No metrics collected yet. Call collect() first."),
        };

        let mut json_data = serde_json::json!({
            "collection_info": {
                "start_time": metrics.collection_start,
                "duration_seconds": metrics.collection_duration_seconds,
            }
        });

        // Add server data
        json_data["server_data"] = self.server_collector.summary();

        // Add space data
        json_data["space_data"] = self.space_collector.as_ref().map(|s| s.summary()).unwrap_or_default();

        // Add participant data
        json_data["participant_data"] = self
            .participant_collector
            .as_ref()
            .map(|p| p.summary())
            .unwrap_or_default();

        json_data
    }

    fn name(&self) -> &'static str {
        "Orchestrator"
    }
}
