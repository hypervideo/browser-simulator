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
use serde_json;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
};

/// Collects and formats space-level data (participant data, CPU, latency, throughput)
pub struct SpaceCollector {
    config: Config,
    clickhouse_client: Client,
    metrics: Option<SpaceData>,
}

impl SpaceCollector {
    pub async fn new(config: Config, clickhouse_client: Client) -> Result<Self> {
        Ok(Self {
            config,
            clickhouse_client,
            metrics: None,
        })
    }

    /// Query global metrics (CPU, memory) for the space
    async fn query_global_metrics(
        &self,
        space_id: &str,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<GlobalMetricRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT 
                space_id,
                ts,
                name,
                labels,
                value
            FROM hyper_session.global_metrics 
            WHERE space_id = ?
            AND ts >= toDateTime64(?, 3, 'UTC')
            AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts
        ";

        let rows: Vec<GlobalMetricRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query CPU usage metrics for the space
    async fn query_cpu_usage(
        &self,
        space_id: &str,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<CpuUsageRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT 
                space_id,
                participant_id,
                ts,
                value
            FROM hyper_session.cpu_usage 
            WHERE space_id = ?
            AND ts >= toDateTime64(?, 3, 'UTC')
            AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts
        ";

        let rows: Vec<CpuUsageRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query throughput metrics for the space
    async fn query_throughput_metrics(
        &self,
        space_id: &str,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<ThroughputRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT 
                participant_id,
                toUInt8(direction) as direction,
                toUInt8(media_type) as media_type,
                ts,
                value
            FROM hyper_session.throughput 
            WHERE space_id = ?
            AND ts >= toDateTime64(?, 3, 'UTC')
            AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY participant_id, direction, ts
        ";

        let rows: Vec<ThroughputRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query latency metrics for the space
    async fn query_latency_metrics(
        &self,
        space_id: &str,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<LatencyMetricRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT 
                receiving_participant_id,
                sending_participant_id,
                toUInt8(media_type) as media_type,
                stream_id,
                ts,
                collect,
                encode,
                send,
                sender,
                relay,
                receiver,
                decode,
                total
            FROM hyper_session.participant_metrics 
            WHERE space_id = ?
            AND ts >= toDateTime64(?, 3, 'UTC')
            AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts ASC, receiving_participant_id, sending_participant_id
        ";

        let rows: Vec<LatencyMetricRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Process global metrics (audio/video processing) for the space
    /// Note: Memory metrics are not available at space level in hyper_session.global_metrics
    /// This table contains audio/video processing metrics instead
    fn process_global_metrics(&self, space_metrics: &mut SpaceData, global_metrics: Vec<GlobalMetricRow>) {
        let mut audio_gap_durations = Vec::new();

        for data_point in global_metrics {
            let name = &data_point.name;
            let value = data_point.value;

            match name.as_str() {
                // Audio processing metrics
                "audio_decoder_output" => {
                    space_metrics.audio_processing_metrics.audio_decoder_output_count += 1;
                }
                "audio_decoder_decode" => {
                    space_metrics.audio_processing_metrics.audio_decoder_decode_count += 1;
                }
                "audio_scheduler_schedule" => {
                    space_metrics.audio_processing_metrics.audio_scheduler_schedule_count += 1;
                }
                "audio_preprocessing_gap_duration" => {
                    audio_gap_durations.push(value);
                }
                "audio_gain_normalization_factor" => {
                    space_metrics.audio_processing_metrics.audio_gain_normalization_factor = value;
                }
                "audio_source_volume" => {
                    space_metrics.audio_processing_metrics.audio_source_volume = value;
                }

                // Video processing metrics
                "video_decoder_output" => {
                    space_metrics.audio_processing_metrics.video_decoder_output_count += 1;
                }
                "video_restore_commit" => {
                    space_metrics.audio_processing_metrics.video_restore_commit_count += 1;
                }
                "video_streams_active" => {
                    space_metrics.audio_processing_metrics.video_streams_active = value as u32;
                }
                "video_send_key_data_request" => {
                    space_metrics.audio_processing_metrics.video_send_key_data_request_count += 1;
                }

                // Network metrics
                "datagrams_receive_expected" => {
                    space_metrics.audio_processing_metrics.datagrams_receive_expected = value as u64;
                }
                "datagrams_receive_lost" => {
                    space_metrics.audio_processing_metrics.datagrams_receive_lost = value as u64;
                }
                "datagrams_receive_received" => {
                    space_metrics.audio_processing_metrics.datagrams_receive_received = value as u64;
                }
                "datagrams_receive_packet_loss_rate" => {
                    space_metrics
                        .audio_processing_metrics
                        .datagrams_receive_packet_loss_rate = value;
                }

                _ => {} // Ignore other metrics
            }
        }

        // Calculate average audio preprocessing gap duration
        if !audio_gap_durations.is_empty() {
            space_metrics
                .audio_processing_metrics
                .audio_preprocessing_gap_duration_avg =
                audio_gap_durations.iter().sum::<f64>() / audio_gap_durations.len() as f64;
        }
    }

    /// Process CPU usage metrics for the space
    fn process_cpu_usage(&self, space_metrics: &mut SpaceData, cpu_usage: Vec<CpuUsageRow>) {
        // Group CPU usage by participant and calculate average per participant
        let mut participant_cpu: HashMap<u16, Vec<f64>> = HashMap::new();

        for data_point in &cpu_usage {
            participant_cpu
                .entry(data_point.participant_id)
                .or_default()
                .push(data_point.value);
        }

        if !participant_cpu.is_empty() {
            // Calculate average CPU per participant, then average across all participants
            let participant_averages: Vec<f64> = participant_cpu
                .values()
                .map(|values| values.iter().sum::<f64>() / values.len() as f64)
                .collect();

            let overall_avg_cpu = participant_averages.iter().sum::<f64>() / participant_averages.len() as f64;

            space_metrics.avg_cpu_usage_percent = overall_avg_cpu * 100.0;
            space_metrics.participant_count = participant_cpu.len() as u32;
        }
    }

    /// Process throughput metrics for the space
    fn process_throughput_metrics(&self, space_metrics: &mut SpaceData, throughput: Vec<ThroughputRow>) {
        let mut total_bytes_sent = 0u64;
        let mut total_bytes_received = 0u64;

        for data_point in throughput {
            let value = data_point.value as u64;

            // direction: 0 = tx, 1 = rx
            // media_type: 0 = audio, 1 = video
            // For now, we'll treat all throughput as bytes (assuming bits_per_second)
            match data_point.direction {
                0 => {
                    // tx
                    total_bytes_sent += value / 8; // Convert bits to bytes
                }
                1 => {
                    // rx
                    total_bytes_received += value / 8; // Convert bits to bytes
                }
                _ => {}
            }
        }

        space_metrics.total_network_bytes_sent = total_bytes_sent;
        space_metrics.total_network_bytes_received = total_bytes_received;
    }

    /// Process latency metrics for the space
    fn process_latency_metrics(&self, space_metrics: &mut SpaceData, latency_metrics: Vec<LatencyMetricRow>) {
        let mut latencies = Vec::new();
        let mut total_latencies = Vec::new();

        for data_point in latency_metrics {
            let ts = DateTime::from_timestamp_millis(data_point.ts).unwrap_or_else(Utc::now);

            // Convert media_type from u8 to String
            let media_type = match data_point.media_type {
                0 => "audio".to_string(),
                1 => "video".to_string(),
                _ => "unknown".to_string(),
            };

            let mut latency = ParticipantLatencyMetrics::new(
                data_point.receiving_participant_id,
                data_point.sending_participant_id,
                media_type,
                data_point.stream_id,
            );
            latency.timestamp = ts;
            latency.collect_latency = data_point.collect;
            latency.encode_latency = data_point.encode;
            latency.send_latency = data_point.send;
            latency.sender_latency = data_point.sender;
            latency.relay_latency = data_point.relay;
            latency.receiver_latency = data_point.receiver;
            latency.decode_latency = data_point.decode;
            latency.total_latency = data_point.total;

            latencies.push(latency);
            total_latencies.push(data_point.total as f64);
        }

        space_metrics.participant_latencies = latencies;
    }
}

impl SpaceCollector {
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

    /// Get color for latency based on threshold
    fn get_latency_color(&self, latency_ms: f64) -> Color {
        if latency_ms < 100.0 {
            Color::Green
        } else if latency_ms < 200.0 {
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

    /// Format number in human readable format
    fn format_number(&self, num: u64) -> String {
        if num >= 1_000_000 {
            format!("{:.1}M", num as f64 / 1_000_000.0)
        } else if num >= 1_000 {
            format!("{:.1}K", num as f64 / 1_000.0)
        } else {
            format!("{}", num)
        }
    }

    /// Print audio/video processing metrics table
    fn print_audio_video_metrics_table(&self, metrics: &AudioVideoProcessingMetrics) -> String {
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![Cell::new("🎵🎬 AUDIO/VIDEO PROCESSING METRICS")
                .add_attribute(Attribute::Bold)
                .fg(Color::Magenta)]);

        // Audio metrics
        table.add_row(vec![
            Cell::new("Audio Decoder Output").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.audio_decoder_output_count)),
        ]);

        table.add_row(vec![
            Cell::new("Audio Decoder Decode").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.audio_decoder_decode_count)),
        ]);

        table.add_row(vec![
            Cell::new("Audio Scheduler Schedule").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.audio_scheduler_schedule_count)),
        ]);

        table.add_row(vec![
            Cell::new("Audio Gap Duration Avg").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.2}ms", metrics.audio_preprocessing_gap_duration_avg)),
        ]);

        // Video metrics
        table.add_row(vec![
            Cell::new("Video Decoder Output").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.video_decoder_output_count)),
        ]);

        table.add_row(vec![
            Cell::new("Video Restore Commit").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.video_restore_commit_count)),
        ]);

        table.add_row(vec![
            Cell::new("Video Streams Active").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", metrics.video_streams_active)),
        ]);

        // Network metrics
        table.add_row(vec![
            Cell::new("Datagrams Expected").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(metrics.datagrams_receive_expected)),
        ]);

        table.add_row(vec![
            Cell::new("Datagrams Lost").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", metrics.datagrams_receive_lost)).fg(if metrics.datagrams_receive_lost == 0 {
                Color::Green
            } else {
                Color::Red
            }),
        ]);

        table.add_row(vec![
            Cell::new("Packet Loss Rate").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.2}%", metrics.datagrams_receive_packet_loss_rate)).fg(
                if metrics.datagrams_receive_packet_loss_rate < 1.0 {
                    Color::Green
                } else {
                    Color::Yellow
                },
            ),
        ]);

        format!("{}\n", table)
    }
}

impl Collector for SpaceCollector {
    fn collect(
        &mut self,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let space_id = self
                .config
                .space_id
                .as_ref()
                .ok_or_else(|| eyre::eyre!("Space ID is required for space-level metrics"))?;

            let mut space_metrics = SpaceData::new(space_id.clone(), self.config.server_url.clone());

            // Query and process all metrics
            let global_metrics = self
                .query_global_metrics(space_id, start_time, duration_seconds)
                .await?;
            self.process_global_metrics(&mut space_metrics, global_metrics);

            let cpu_usage = self.query_cpu_usage(space_id, start_time, duration_seconds).await?;
            self.process_cpu_usage(&mut space_metrics, cpu_usage);

            let throughput_metrics = self
                .query_throughput_metrics(space_id, start_time, duration_seconds)
                .await?;
            self.process_throughput_metrics(&mut space_metrics, throughput_metrics);

            let latency_metrics = self
                .query_latency_metrics(space_id, start_time, duration_seconds)
                .await?;
            self.process_latency_metrics(&mut space_metrics, latency_metrics);

            // Store metrics internally
            self.metrics = Some(space_metrics);
            Ok(())
        })
    }

    fn format(&self) -> String {
        let metrics = match &self.metrics {
            Some(m) => m,
            None => return "No metrics collected yet. Call collect() first.".to_string(),
        };

        let mut output = String::new();

        // Main space metrics table
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![Cell::new(format!(
                "👥 SPACE METRICS (Space: {})",
                metrics.space_id
            ))
            .add_attribute(Attribute::Bold)
            .fg(Color::Green)]);

        // Add main metrics rows
        table.add_row(vec![
            Cell::new("Participants").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", metrics.participant_count)),
        ]);

        table.add_row(vec![
            Cell::new("Avg CPU Usage").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}%", metrics.avg_cpu_usage_percent))
                .fg(self.get_cpu_color(metrics.avg_cpu_usage_percent)),
        ]);

        table.add_row(vec![
            Cell::new("Network Sent").add_attribute(Attribute::Bold),
            Cell::new(self.format_bytes(metrics.total_network_bytes_sent).to_string()),
        ]);

        table.add_row(vec![
            Cell::new("Network Received").add_attribute(Attribute::Bold),
            Cell::new(self.format_bytes(metrics.total_network_bytes_received).to_string()),
        ]);

        table.add_row(vec![
            Cell::new("Avg Latency").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}ms", metrics.avg_latency_ms()))
                .fg(self.get_latency_color(metrics.avg_latency_ms())),
        ]);

        table.add_row(vec![
            Cell::new("Max Latency").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}ms", metrics.max_latency_ms()))
                .fg(self.get_latency_color(metrics.max_latency_ms())),
        ]);

        table.add_row(vec![
            Cell::new("P95 Latency").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}ms", metrics.p95_latency_ms()))
                .fg(self.get_latency_color(metrics.p95_latency_ms())),
        ]);

        output.push_str(&format!("{}\n", table));

        // Add audio/video processing metrics table
        output.push_str(&self.print_audio_video_metrics_table(&metrics.audio_processing_metrics));

        output
    }

    fn summary(&self) -> serde_json::Value {
        let metrics = match &self.metrics {
            Some(m) => m,
            None => return serde_json::json!({"error": "No metrics collected yet"}),
        };

        serde_json::json!({
            "space_id": metrics.space_id,
            "server_url": metrics.server_url,
            "timestamp": metrics.timestamp,
            "participants": {
                "count": metrics.participant_count
            },
            "cpu": {
                "total_usage_percent": metrics.total_cpu_usage_percent,
                "avg_usage_percent": metrics.avg_cpu_usage_percent,
                "max_usage_percent": metrics.max_cpu_usage_percent
            },
            "network": {
                "bytes_sent": metrics.total_network_bytes_sent,
                "bytes_received": metrics.total_network_bytes_received,
                "errors": metrics.total_network_errors
            },
            "latency": {
                "avg_ms": metrics.avg_latency_ms(),
                "max_ms": metrics.max_latency_ms(),
                "p95_ms": metrics.p95_latency_ms(),
                "participant_count": metrics.participant_latencies.len()
            },
            "audio_video_processing": {
                "audio_decoder_output_count": metrics.audio_processing_metrics.audio_decoder_output_count,
                "audio_decoder_decode_count": metrics.audio_processing_metrics.audio_decoder_decode_count,
                "audio_scheduler_schedule_count": metrics.audio_processing_metrics.audio_scheduler_schedule_count,
                "audio_preprocessing_gap_duration_avg": metrics.audio_processing_metrics.audio_preprocessing_gap_duration_avg,
                "audio_gain_normalization_factor": metrics.audio_processing_metrics.audio_gain_normalization_factor,
                "audio_source_volume": metrics.audio_processing_metrics.audio_source_volume,
                "video_decoder_output_count": metrics.audio_processing_metrics.video_decoder_output_count,
                "video_restore_commit_count": metrics.audio_processing_metrics.video_restore_commit_count,
                "video_streams_active": metrics.audio_processing_metrics.video_streams_active,
                "video_send_key_data_request_count": metrics.audio_processing_metrics.video_send_key_data_request_count,
                "datagrams_receive_expected": metrics.audio_processing_metrics.datagrams_receive_expected,
                "datagrams_receive_lost": metrics.audio_processing_metrics.datagrams_receive_lost,
                "datagrams_receive_received": metrics.audio_processing_metrics.datagrams_receive_received,
                "datagrams_receive_packet_loss_rate": metrics.audio_processing_metrics.datagrams_receive_packet_loss_rate,
            }
        })
    }

    fn name(&self) -> &'static str {
        "SpaceCollector"
    }
}

// ClickHouse row structs
#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct GlobalMetricRow {
    #[allow(dead_code)]
    space_id: String,
    #[allow(dead_code)]
    ts: i64, // milliseconds since epoch
    name: String,
    #[allow(dead_code)]
    labels: Vec<(String, String)>,
    value: f64,
}

#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct CpuUsageRow {
    #[allow(dead_code)]
    space_id: String,
    participant_id: u16,
    #[allow(dead_code)]
    ts: i64, // milliseconds since epoch
    value: f64,
}

#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct ThroughputRow {
    #[allow(dead_code)]
    participant_id: u16,
    direction: u8, // 0 = tx, 1 = rx
    #[allow(dead_code)]
    media_type: u8, // 0 = audio, 1 = video
    #[allow(dead_code)]
    ts: i64, // milliseconds since epoch
    value: f64,
}

#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct LatencyMetricRow {
    receiving_participant_id: u16,
    sending_participant_id: u16,
    media_type: u8, // 0 = audio, 1 = video
    stream_id: u8,
    #[allow(dead_code)]
    ts: i64, // milliseconds since epoch
    collect: u16,
    encode: u16,
    send: u16,
    sender: u16,
    relay: u16,
    receiver: u16,
    decode: u16,
    total: u16,
}
