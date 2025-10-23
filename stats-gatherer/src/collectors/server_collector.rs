use crate::{
    collectors::Collector,
    config::Config,
    metrics::*,
};
use chrono::{
    DateTime,
    Utc,
};
use clickhouse::Client;
use comfy_table::{
    presets,
    Attribute,
    Cell,
    Color,
    ContentArrangement,
    Table,
};
use eyre::Result;
use reqwest::Client as HttpClient;
use serde_json;
use std::{
    collections::{
        BTreeMap,
        HashMap,
    },
    future::Future,
    pin::Pin,
    time::Duration,
};

/// Collects and formats server-level data (CPU, memory, network, health)
pub struct ServerCollector {
    config: Config,
    clickhouse_client: Client,
    http_client: HttpClient,
    metrics: Option<ServerData>,
}

impl ServerCollector {
    pub async fn new(config: Config, clickhouse_client: Client, http_client: HttpClient) -> Result<Self> {
        Ok(Self {
            config,
            clickhouse_client,
            http_client,
            metrics: None,
        })
    }

    /// Perform HTTP health check on the server
    async fn perform_health_check(&self, server_url: &str) -> Result<(f64, u16)> {
        let start = std::time::Instant::now();
        let response = self
            .http_client
            .get(format!("{}/health", server_url))
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        let response_time = start.elapsed().as_millis() as f64;
        let status_code = response.status().as_u16();

        Ok((response_time, status_code))
    }

    /// Query server metrics from ClickHouse
    async fn query_server_metrics(
        &self,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<ServerMetricRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT 
                ts,
                name,
                labels,
                value,
                metric_type
            FROM hyper_session.server_metrics 
            WHERE ts >= toDateTime64(?, 3, 'UTC')
            AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts ASC, name
            LIMIT 1000000
        ";

        let rows: Vec<ServerMetricRow> = self
            .clickhouse_client
            .query(query)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query participant join events for a given time range
    async fn query_participant_joins(
        &self,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<ParticipantJoinEventRow>> {
        // Only query if we have a space ID
        let space_id = match &self.config.space_id {
            Some(id) => id,
            None => return Ok(Vec::new()), // No space ID means no participant data
        };

        let end_time = start_time + chrono::Duration::seconds(duration_seconds);
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT DISTINCT receiving_participant_id AS participant_id, 
                   min(ts) as first_seen
            FROM hyper_session.participant_metrics
            WHERE space_id = ?
              AND ts >= toDateTime64(?, 3, 'UTC')
              AND ts <= toDateTime64(?, 3, 'UTC')
            GROUP BY receiving_participant_id
            UNION DISTINCT
            SELECT DISTINCT sending_participant_id AS participant_id,
                   min(ts) as first_seen
            FROM hyper_session.participant_metrics
            WHERE space_id = ?
              AND ts >= toDateTime64(?, 3, 'UTC')
              AND ts <= toDateTime64(?, 3, 'UTC')
            GROUP BY sending_participant_id
            ORDER BY first_seen
        ";

        let rows: Vec<ParticipantJoinEventRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Process raw server metrics into structured data
    fn process_server_metrics(&self, server_metrics: Vec<ServerMetricRow>) -> ServerData {
        let mut metrics = ServerData::new("".to_string());

        // Group metrics by name for processing
        let mut cpu_usage_values = Vec::new();
        let mut cpu_data_points = Vec::new(); // Store CPU data with timestamps
        let mut memory_bytes_values = Vec::new(); // Collect all memory byte values
        let mut memory_percent_values = Vec::new();
        let mut load_average_values = Vec::new();

        // Group network samples by interface + direction for proper time series handling
        let mut network_bytes_samples: HashMap<String, BTreeMap<i64, f64>> = HashMap::new();
        let mut network_packets_samples: HashMap<String, BTreeMap<i64, f64>> = HashMap::new();

        for data_point in server_metrics {
            let name = &data_point.name;
            let value = data_point.value;

            match name.as_str() {
                "server_system_cpu_usage_percent" => {
                    cpu_usage_values.push(value);
                    // Store CPU data point with timestamp for graph
                    if let Some(timestamp) = DateTime::from_timestamp_millis(data_point.ts) {
                        cpu_data_points.push(crate::metrics::CpuDataPoint {
                            timestamp,
                            cpu_usage_percent: value,
                        });
                    }
                }
                "server_system_memory_bytes" => {
                    memory_bytes_values.push(value);
                }
                "server_system_memory_percent" => {
                    memory_percent_values.push(value);
                }
                "server_system_network_bytes" => {
                    // Extract interface and direction from labels to create stable key
                    let interface = data_point
                        .labels
                        .iter()
                        .find(|(key, _)| key == "interface")
                        .map(|(_, value)| value.as_str())
                        .unwrap_or("unknown");

                    let direction = data_point
                        .labels
                        .iter()
                        .find(|(key, _)| key == "direction")
                        .map(|(_, value)| value.as_str())
                        .unwrap_or("unknown");

                    let series_key = format!("{}_{}", interface, direction);
                    network_bytes_samples
                        .entry(series_key)
                        .or_insert_with(BTreeMap::new)
                        .insert(data_point.ts, value);
                }
                "server_system_network_packets" => {
                    // Extract interface and direction from labels to create stable key
                    let interface = data_point
                        .labels
                        .iter()
                        .find(|(key, _)| key == "interface")
                        .map(|(_, value)| value.as_str())
                        .unwrap_or("unknown");

                    let direction = data_point
                        .labels
                        .iter()
                        .find(|(key, _)| key == "direction")
                        .map(|(_, value)| value.as_str())
                        .unwrap_or("unknown");

                    let series_key = format!("{}_{}", interface, direction);
                    network_packets_samples
                        .entry(series_key)
                        .or_insert_with(BTreeMap::new)
                        .insert(data_point.ts, value);
                }
                "server_system_load_average" => {
                    load_average_values.push(value);
                }
                _ => {}
            }
        }

        // Calculate averages and totals
        if !cpu_usage_values.is_empty() {
            metrics.cpu_usage_percent = cpu_usage_values.iter().sum::<f64>() / cpu_usage_values.len() as f64;
        } else {
            metrics.cpu_usage_percent = 0.0;
        }

        // Store CPU data points for graph
        metrics.cpu_data_points = cpu_data_points;

        // Process memory metrics
        if !memory_bytes_values.is_empty() {
            // Find the largest value as total memory (appears to be around 2GB)
            let total_memory: f64 = memory_bytes_values.iter().fold(0.0, |acc: f64, x: &f64| acc.max(*x));
            metrics.memory_total_bytes = total_memory as u64;

            // Calculate used memory from percentage if available
            if !memory_percent_values.is_empty() {
                let avg_percent = memory_percent_values.iter().sum::<f64>() / memory_percent_values.len() as f64;
                metrics.memory_used_bytes = (total_memory * avg_percent / 100.0) as u64;
                metrics.memory_usage_percent = avg_percent;
            } else {
                // Fallback: estimate used memory from the smaller values
                let used_memory: f64 = memory_bytes_values
                    .iter()
                    .filter(|&&x: &&f64| x < total_memory * 0.9) // Exclude the total memory value
                    .fold(0.0, |acc: f64, x: &f64| acc.max(*x));
                metrics.memory_used_bytes = used_memory as u64;
                metrics.memory_usage_percent = (used_memory / total_memory) * 100.0;
            }
        }

        if !load_average_values.is_empty() {
            metrics.cpu_load_average = load_average_values.iter().sum::<f64>() / load_average_values.len() as f64;
        }

        // Calculate network deltas per time series to handle counter resets properly
        let mut total_bytes_tx = 0u64;
        let mut total_bytes_rx = 0u64;
        let mut total_packets_tx = 0u64;
        let mut total_packets_rx = 0u64;

        // Process network bytes deltas
        for (series_key, samples) in &network_bytes_samples {
            let delta = self.calculate_series_delta(samples);
            if series_key.ends_with("_tx") {
                total_bytes_tx += delta;
            } else if series_key.ends_with("_rx") {
                total_bytes_rx += delta;
            }
        }

        // Process network packets deltas
        for (series_key, samples) in &network_packets_samples {
            let delta = self.calculate_series_delta(samples);
            if series_key.ends_with("_tx") {
                total_packets_tx += delta;
            } else if series_key.ends_with("_rx") {
                total_packets_rx += delta;
            }
        }

        metrics.network_bytes_sent = total_bytes_tx;
        metrics.network_bytes_received = total_bytes_rx;
        metrics.network_packets_sent = total_packets_tx;
        metrics.network_packets_received = total_packets_rx;

        // Calculate memory usage percentage
        if metrics.memory_total_bytes > 0 {
            metrics.memory_usage_percent =
                (metrics.memory_used_bytes as f64 / metrics.memory_total_bytes as f64) * 100.0;
        }

        metrics
    }

    /// Calculate delta for a single time series, handling counter resets
    fn calculate_series_delta(&self, samples: &BTreeMap<i64, f64>) -> u64 {
        if samples.len() < 2 {
            return 0;
        }

        let mut total_delta = 0u64;
        let mut prev_value = None;

        // Iterate through samples in chronological order
        for (_, &value) in samples {
            if let Some(prev) = prev_value {
                let diff = value - prev;
                if diff >= 0.0 {
                    // Normal case: positive difference
                    total_delta += diff as u64;
                } else {
                    // Counter reset: treat negative difference as the later value
                    total_delta += value as u64;
                }
            }
            prev_value = Some(value);
        }

        total_delta
    }
}

impl Collector for ServerCollector {
    fn collect(
        &mut self,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut metrics = ServerData::new(self.config.server_url.clone());

            // Perform HTTP health check
            let health_result = self.perform_health_check(&self.config.server_url).await;
            if let Ok((response_time, status_code)) = health_result {
                metrics.response_time_ms = response_time;
                metrics.status_code = Some(status_code);
                // Only consider 2xx status codes as healthy
                metrics.is_healthy = status_code >= 200 && status_code <= 299;
            } else {
                // Network error or timeout - mark as unhealthy
                metrics.is_healthy = false;
            }

            // Query server metrics from ClickHouse
            let server_metrics = self.query_server_metrics(start_time, duration_seconds).await?;
            let processed_metrics = self.process_server_metrics(server_metrics);

            // Query participant join events
            let participant_joins = self.query_participant_joins(start_time, duration_seconds).await?;

            // Merge processed metrics with health data
            metrics.cpu_usage_percent = processed_metrics.cpu_usage_percent;
            metrics.cpu_load_average = processed_metrics.cpu_load_average;
            metrics.cpu_data_points = processed_metrics.cpu_data_points; // Copy CPU data points for graph
            metrics.memory_used_bytes = processed_metrics.memory_used_bytes;
            metrics.memory_total_bytes = processed_metrics.memory_total_bytes;
            metrics.memory_usage_percent = processed_metrics.memory_usage_percent;
            metrics.network_bytes_sent = processed_metrics.network_bytes_sent;
            metrics.network_bytes_received = processed_metrics.network_bytes_received;
            metrics.network_packets_sent = processed_metrics.network_packets_sent;
            metrics.network_packets_received = processed_metrics.network_packets_received;

            // Store participant join events
            metrics.participant_join_events = participant_joins
                .into_iter()
                .filter_map(|event| {
                    DateTime::from_timestamp_millis(event.first_seen).map(|first_seen| {
                        crate::metrics::ParticipantJoinEvent {
                            participant_id: event.participant_id,
                            first_seen,
                        }
                    })
                })
                .collect();

            // Store metrics internally
            self.metrics = Some(metrics);
            Ok(())
        })
    }

    fn format(&self) -> String {
        let metrics = match &self.metrics {
            Some(m) => m,
            None => return "No metrics collected yet. Call collect() first.".to_string(),
        };

        let mut output = String::new();

        // Create the beautiful table display
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![Cell::new("🖥️  SERVER METRICS       ")
                .add_attribute(Attribute::Bold)
                .fg(Color::Cyan)]);

        table.add_row(vec![
            Cell::new("Health Status").add_attribute(Attribute::Bold),
            Cell::new(format!(
                "{} ({:.0}ms)",
                if metrics.is_healthy {
                    "✅ Healthy"
                } else {
                    "❌ Unhealthy"
                },
                metrics.response_time_ms
            ))
            .fg(if metrics.is_healthy { Color::Green } else { Color::Red }),
        ]);

        table.add_row(vec![
            Cell::new("CPU Usage").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}%", metrics.cpu_usage_percent)).fg(self.get_cpu_color(metrics.cpu_usage_percent)),
        ]);

        table.add_row(vec![
            Cell::new("Memory Usage").add_attribute(Attribute::Bold),
            Cell::new(format!(
                "{} / {} ({:.1}%)",
                self.format_bytes(metrics.memory_used_bytes),
                self.format_bytes(metrics.memory_total_bytes),
                metrics.memory_usage_percent
            ))
            .fg(self.get_memory_color(metrics.memory_usage_percent)),
        ]);

        table.add_row(vec![
            Cell::new("Network Sent").add_attribute(Attribute::Bold),
            Cell::new(self.format_bytes(metrics.network_bytes_sent).to_string()),
        ]);

        table.add_row(vec![
            Cell::new("Network Received").add_attribute(Attribute::Bold),
            Cell::new(self.format_bytes(metrics.network_bytes_received).to_string()),
        ]);

        table.add_row(vec![
            Cell::new("Packets Sent").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.network_packets_sent).to_string()),
        ]);

        table.add_row(vec![
            Cell::new("Packets Received").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.network_packets_received).to_string()),
        ]);

        output.push_str(&format!("{}\n", table));

        // Add CPU bar chart
        output.push_str(&self.format_cpu_bar_chart(&metrics.cpu_data_points, &metrics.participant_join_events));

        output
    }

    fn summary(&self) -> serde_json::Value {
        let metrics = match &self.metrics {
            Some(m) => m,
            None => return serde_json::json!({"error": "No metrics collected yet"}),
        };

        serde_json::json!({
            "server_url": metrics.server_url,
            "timestamp": metrics.timestamp,
            "health": {
                "is_healthy": metrics.is_healthy,
                "response_time_ms": metrics.response_time_ms,
                "status_code": metrics.status_code
            },
            "cpu": {
                "usage_percent": metrics.cpu_usage_percent,
                "load_average": metrics.cpu_load_average
            },
            "memory": {
                "used_bytes": metrics.memory_used_bytes,
                "total_bytes": metrics.memory_total_bytes,
                "usage_percent": metrics.memory_usage_percent,
                "used_gb": metrics.memory_usage_gb(),
                "total_gb": metrics.memory_total_gb()
            },
            "network": {
                "bytes_sent": metrics.network_bytes_sent,
                "bytes_received": metrics.network_bytes_received,
                "packets_sent": metrics.network_packets_sent,
                "packets_received": metrics.network_packets_received,
                "errors": metrics.network_errors
            }
        })
    }

    fn name(&self) -> &'static str {
        "ServerCollector"
    }
}

impl ServerCollector {
    /// Get color for CPU usage based on threshold
    fn get_cpu_color(&self, cpu_usage: f64) -> Color {
        if cpu_usage < 50.0 {
            Color::Green
        } else if cpu_usage < 80.0 {
            Color::Yellow
        } else {
            Color::Red
        }
    }

    /// Get color for memory usage based on threshold
    fn get_memory_color(&self, memory_usage: f64) -> Color {
        if memory_usage < 60.0 {
            Color::Green
        } else if memory_usage < 85.0 {
            Color::Yellow
        } else {
            Color::Red
        }
    }

    /// Format bytes in human readable format
    fn format_bytes(&self, bytes: u64) -> String {
        if bytes >= 1024 * 1024 * 1024 {
            format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if bytes >= 1024 * 1024 {
            format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes >= 1024 {
            format!("{:.1}KB", bytes as f64 / 1024.0)
        } else {
            format!("{}B", bytes)
        }
    }

    /// Format numbers in human readable format
    fn format_number(&self, num: u64) -> String {
        if num >= 1_000_000 {
            format!("{:.1}M", num as f64 / 1_000_000.0)
        } else if num >= 1_000 {
            format!("{:.1}K", num as f64 / 1_000.0)
        } else {
            format!("{}", num)
        }
    }

    /// Format CPU usage as a table with timestamp, bar, and percentage columns
    fn format_cpu_bar_chart(
        &self,
        cpu_data_points: &[crate::metrics::CpuDataPoint],
        participant_joins: &[crate::metrics::ParticipantJoinEvent],
    ) -> String {
        let mut output = String::new();
        output.push_str("\n📊 CPU USAGE OVER TIME\n");

        // If no data points, show a message
        if cpu_data_points.is_empty() {
            let mut table = Table::new();
            table
                .load_preset(presets::UTF8_FULL)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(vec![
                    Cell::new("No data available for the time period").add_attribute(Attribute::Bold)
                ]);
            output.push_str(&format!("{}\n", table));
            return output;
        }

        // Extract CPU values and timestamps
        let cpu_values: Vec<f64> = cpu_data_points.iter().map(|dp| dp.cpu_usage_percent).collect();
        let timestamps: Vec<String> = cpu_data_points
            .iter()
            .map(|dp| dp.timestamp.format("%H:%M:%S").to_string())
            .collect();

        // Calculate statistics from original data
        let min_val = cpu_values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max_val = cpu_values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let avg = if cpu_values.len() > 0 {
            cpu_values.iter().sum::<f64>() / cpu_values.len() as f64
        } else {
            0.0
        };

        // Group data if we have more than 10 data points
        let (display_values, display_timestamps) = if cpu_values.len() > 10 {
            let max_groups = 10;
            let group_size = (cpu_values.len() as f64 / max_groups as f64).ceil() as usize;
            let mut grouped_values = Vec::new();
            let mut grouped_timestamps = Vec::new();

            for i in (0..cpu_values.len()).step_by(group_size) {
                let end_idx = (i + group_size).min(cpu_values.len());
                let group_avg = if end_idx > i {
                    cpu_values[i..end_idx].iter().sum::<f64>() / (end_idx - i) as f64
                } else {
                    0.0
                };
                grouped_values.push(group_avg);

                // Create time range for the group
                let start_time = &timestamps[i];
                let end_time = if end_idx < timestamps.len() {
                    &timestamps[end_idx - 1]
                } else {
                    &timestamps[timestamps.len() - 1]
                };

                if start_time == end_time {
                    grouped_timestamps.push(start_time.clone());
                } else {
                    grouped_timestamps.push(format!("{}~{}", start_time, end_time));
                }
            }
            (grouped_values, grouped_timestamps)
        } else {
            (cpu_values, timestamps)
        };

        // Create table with comfy_table
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new("Timestamp").add_attribute(Attribute::Bold),
                Cell::new("Joined").add_attribute(Attribute::Bold),
                Cell::new("Bar").add_attribute(Attribute::Bold),
                Cell::new("%").add_attribute(Attribute::Bold),
            ]);

        // Add data rows using grouped data
        for (timestamp_index, (value, timestamp)) in display_values.iter().zip(display_timestamps.iter()).enumerate() {
            // Calculate bar length (max 50 characters)
            let bar_length = if max_val > 0.0 {
                ((value / max_val) * 50.0).round() as usize
            } else {
                0
            };
            let bar_length = bar_length.max(1); // At least 1 character wide

            // Create the bar with solid block character
            let bar = "█".repeat(bar_length);

            // Apply color based on CPU usage
            let color = match *value {
                v if v < 30.0 => Color::Green,
                v if v < 60.0 => Color::Yellow,
                v if v < 80.0 => Color::Magenta,
                _ => Color::Red,
            };

            // Calculate participants joined in this time interval
            let participants_joined =
                self.count_participants_in_time_interval(participant_joins, timestamp_index, cpu_data_points);

            table.add_row(vec![
                Cell::new(timestamp),
                Cell::new(format!("{}", participants_joined)),
                Cell::new(&bar).fg(color),
                Cell::new(format!("{:.1}%", value)),
            ]);
        }

        // Add footer rows with statistics
        let start_time = cpu_data_points
            .first()
            .map(|dp| dp.timestamp.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "N/A".to_string());
        let end_time = cpu_data_points
            .last()
            .map(|dp| dp.timestamp.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "N/A".to_string());

        let time_range = format!(
            "Time Range: {} → {} │ Data Points: {}",
            start_time,
            end_time,
            cpu_data_points.len()
        );
        table.add_row(vec!["", "", &time_range, ""]);

        let stats = format!("Min: {:.1}% │ Avg: {:.1}% │ Max: {:.1}%", min_val, avg, max_val);
        table.add_row(vec!["", "", &stats, ""]);

        output.push_str(&format!("{}\n", table));

        output
    }

    /// Count participants that joined in a specific time interval
    fn count_participants_in_time_interval(
        &self,
        participant_joins: &[crate::metrics::ParticipantJoinEvent],
        timestamp_index: usize,
        cpu_data_points: &[crate::metrics::CpuDataPoint],
    ) -> usize {
        if participant_joins.is_empty() || cpu_data_points.is_empty() {
            return 0;
        }

        // Determine the time interval for this timestamp
        let group_size = if cpu_data_points.len() > 10 {
            (cpu_data_points.len() as f64 / 10.0).ceil() as usize
        } else {
            1
        };

        let start_idx = timestamp_index * group_size;
        let end_idx = ((timestamp_index + 1) * group_size).min(cpu_data_points.len());

        let interval_start = if start_idx < cpu_data_points.len() {
            cpu_data_points[start_idx].timestamp
        } else {
            return 0;
        };

        let interval_end = if end_idx > 0 && end_idx - 1 < cpu_data_points.len() {
            cpu_data_points[end_idx - 1].timestamp
        } else {
            return 0;
        };

        // Count participants that joined during this time interval
        participant_joins
            .iter()
            .filter(|join_event| join_event.first_seen >= interval_start && join_event.first_seen <= interval_end)
            .count()
    }
}

// ClickHouse row struct for server metrics
#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct ServerMetricRow {
    #[allow(dead_code)]
    ts: i64, // milliseconds since epoch
    name: String,
    labels: Vec<(String, String)>,
    value: f64,
    #[allow(dead_code)]
    metric_type: u8,
}

// ClickHouse row struct for participant join events
#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct ParticipantJoinEventRow {
    participant_id: u16,
    first_seen: i64, // milliseconds since epoch
}
