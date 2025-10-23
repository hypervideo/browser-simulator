use crate::{
    collectors::Collector,
    config::Config,
};
use chrono::{
    DateTime,
    Utc,
};
use color_eyre::Result;
use comfy_table::{
    presets,
    Attribute,
    Cell,
    Color,
    ContentArrangement,
    Table,
};
use serde_json;
use std::{
    future::Future,
    pin::Pin,
};

/// Participant-level data collector
pub struct ParticipantCollector {
    config: Config,
    clickhouse_client: clickhouse::Client,
    metrics: Option<Vec<ParticipantData>>,
}

#[derive(Debug, Clone)]
pub struct ParticipantData {
    pub participant_id: u16,
    pub space_id: String,
    pub server_url: String,
    pub timestamp: DateTime<Utc>,
    pub join_time: DateTime<Utc>,
    pub leave_time: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i64>,
    pub avg_cpu_usage: f64,
    pub max_cpu_usage: f64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_packets_sent: u64,
    pub total_packets_received: u64,
    pub avg_latency_ms: f64,
    pub max_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub audio_metrics: ParticipantAudioVideoMetrics,
    pub video_metrics: ParticipantAudioVideoMetrics,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ParticipantAudioVideoMetrics {
    pub decoder_output_count: u64,
    pub decoder_decode_count: u64,
    pub scheduler_schedule_count: u64,
    pub restore_commit_count: u64,
    pub preprocessing_gap_duration_avg: f64,
    pub gain_normalization_factor: f64,
    pub source_volume: f64,
    pub streams_active: u16,
    pub datagrams_expected: u64,
    pub datagrams_lost: u64,
    pub datagrams_received: u64,
    pub packet_loss_rate: f64,
}

impl ParticipantCollector {
    pub async fn new(config: Config, clickhouse_client: clickhouse::Client) -> Result<Self> {
        Ok(Self {
            config,
            clickhouse_client,
            metrics: None,
        })
    }

    /// Query participant timeline data (join/leave times)
    async fn query_participant_timeline(
        &self,
        space_id: &str,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<ParticipantTimelineRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);

        // Use the same approach as the backend - proper query binding and date formatting
        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT participant_id,
                   min(ts) as first_seen,
                   max(ts) as last_seen,
                   count(*) as data_points
            FROM hyper_session.cpu_usage
            WHERE space_id = ?
              AND ts >= toDateTime64(?, 3, 'UTC')
              AND ts <= toDateTime64(?, 3, 'UTC')
            GROUP BY participant_id
            ORDER BY participant_id
        ";

        let rows: Vec<ParticipantTimelineRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query participant CPU usage
    async fn query_participant_cpu_usage(
        &self,
        space_id: &str,
        participant_id: u16,
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
                AND participant_id = ?
                AND ts >= toDateTime64(?, 3, 'UTC')
                AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts
        ";

        let rows: Vec<CpuUsageRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(participant_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query participant throughput metrics
    async fn query_participant_throughput(
        &self,
        space_id: &str,
        participant_id: u16,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<ThroughputRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);

        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let query = "
            SELECT 
                participant_id,
                direction,
                media_type,
                ts,
                value
            FROM hyper_session.throughput
            WHERE space_id = ?
                AND participant_id = ?
                AND ts >= toDateTime64(?, 3, 'UTC')
                AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts
        ";

        let rows: Vec<ThroughputRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(participant_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query participant latency metrics
    async fn query_participant_latency(
        &self,
        space_id: &str,
        participant_id: u16,
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
                media_type,
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
                AND (receiving_participant_id = ? OR sending_participant_id = ?)
                AND ts >= toDateTime64(?, 3, 'UTC')
                AND ts <= toDateTime64(?, 3, 'UTC')
            ORDER BY ts
        ";

        let rows = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(participant_id)
            .bind(participant_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Query participant audio/video processing metrics
    async fn query_participant_audio_video_metrics(
        &self,
        space_id: &str,
        participant_id: u16,
        start_time: DateTime<Utc>,
        duration_seconds: i64,
    ) -> Result<Vec<GlobalMetricRow>> {
        let end_time = start_time + chrono::Duration::seconds(duration_seconds);

        let since_str = start_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let until_str = end_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        // Follow the backend approach - filter by participant_id column, not by labels
        let query = "
            SELECT 
                space_id,
                ts,
                name,
                labels,
                value
            FROM hyper_session.global_metrics
            WHERE space_id = ?
                AND participant_id = ?
                AND ts >= toDateTime64(?, 3, 'UTC')
                AND ts <= toDateTime64(?, 3, 'UTC')
                AND (
                    name LIKE 'audio_%' OR 
                    name LIKE 'video_%' OR 
                    name LIKE 'datagrams_%'
                )
            ORDER BY ts
        ";

        let rows: Vec<GlobalMetricRow> = self
            .clickhouse_client
            .query(query)
            .bind(space_id)
            .bind(participant_id)
            .bind(&since_str)
            .bind(&until_str)
            .fetch_all()
            .await?;

        Ok(rows)
    }

    /// Process participant timeline data
    fn process_participant_timeline(&self, timeline: &[ParticipantTimelineRow]) -> Vec<ParticipantData> {
        let mut participants = Vec::new();

        for row in timeline {
            let join_time = match DateTime::from_timestamp_millis(row.first_seen) {
                Some(ts) => ts,
                None => continue,
            };
            let leave_time = if row.last_seen < Utc::now().timestamp_millis() {
                DateTime::from_timestamp_millis(row.last_seen)
            } else {
                None
            };

            let duration_seconds = leave_time.map(|leave| (leave - join_time).num_seconds());

            let participant = ParticipantData {
                participant_id: row.participant_id,
                space_id: self.config.space_id.as_ref().unwrap().clone(),
                server_url: self.config.server_url.clone(),
                timestamp: Utc::now(),
                join_time,
                leave_time,
                duration_seconds,
                avg_cpu_usage: 0.0,
                max_cpu_usage: 0.0,
                total_bytes_sent: 0,
                total_bytes_received: 0,
                total_packets_sent: 0,
                total_packets_received: 0,
                avg_latency_ms: 0.0,
                max_latency_ms: 0.0,
                p95_latency_ms: 0.0,
                audio_metrics: ParticipantAudioVideoMetrics::default(),
                video_metrics: ParticipantAudioVideoMetrics::default(),
            };

            participants.push(participant);
        }

        participants
    }

    /// Process CPU usage data for a participant
    fn process_cpu_usage(&self, participant: &mut ParticipantData, cpu_data: &[CpuUsageRow]) {
        if cpu_data.is_empty() {
            return;
        }

        let cpu_percentages: Vec<f64> = cpu_data.iter().map(|row| row.value * 100.0).collect();

        participant.avg_cpu_usage = cpu_percentages.iter().sum::<f64>() / cpu_percentages.len() as f64;
        participant.max_cpu_usage = cpu_percentages.iter().fold(0.0, |acc, &x| acc.max(x));
    }

    /// Process throughput data for a participant
    fn process_throughput(&self, participant: &mut ParticipantData, throughput_data: &[ThroughputRow]) {
        for row in throughput_data {
            match row.direction {
                0 => {
                    // TX (sent)
                    participant.total_bytes_sent += row.value as u64;
                    participant.total_packets_sent += 1;
                }
                1 => {
                    // RX (received)
                    participant.total_bytes_received += row.value as u64;
                    participant.total_packets_received += 1;
                }
                _ => {}
            }
        }
    }

    /// Process latency data for a participant
    fn process_latency(&self, participant: &mut ParticipantData, latency_data: &[LatencyMetricRow]) {
        if latency_data.is_empty() {
            return;
        }

        let values: Vec<f64> = latency_data.iter().map(|row| row.total as f64).collect();
        participant.avg_latency_ms = values.iter().sum::<f64>() / values.len() as f64;
        participant.max_latency_ms = values.iter().fold(0.0, |acc, &x| acc.max(x));

        // Calculate P95
        let mut sorted_values = values.clone();
        sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95_index = (sorted_values.len() as f64 * 0.95) as usize;
        participant.p95_latency_ms = sorted_values[p95_index.min(sorted_values.len() - 1)];
    }

    /// Process audio/video metrics for a participant
    fn process_audio_video_metrics(&self, participant: &mut ParticipantData, metrics_data: &[GlobalMetricRow]) {
        for row in metrics_data {
            match row.name.as_str() {
                "audio_decoder_output" => participant.audio_metrics.decoder_output_count = row.value as u64,
                "audio_decoder_decode" => participant.audio_metrics.decoder_decode_count = row.value as u64,
                "audio_scheduler_schedule" => participant.audio_metrics.scheduler_schedule_count = row.value as u64,
                "audio_preprocessing_gap_duration_avg" => {
                    participant.audio_metrics.preprocessing_gap_duration_avg = row.value
                }
                "audio_gain_normalization_factor" => participant.audio_metrics.gain_normalization_factor = row.value,
                "audio_source_volume" => participant.audio_metrics.source_volume = row.value,
                "video_decoder_output" => participant.video_metrics.decoder_output_count = row.value as u64,
                "video_restore_commit" => participant.video_metrics.restore_commit_count = row.value as u64,
                "video_streams_active" => participant.video_metrics.streams_active = row.value as u16,
                "datagrams_receive_expected" => {
                    participant.audio_metrics.datagrams_expected = row.value as u64;
                    participant.video_metrics.datagrams_expected = row.value as u64;
                }
                "datagrams_receive_lost" => {
                    participant.audio_metrics.datagrams_lost = row.value as u64;
                    participant.video_metrics.datagrams_lost = row.value as u64;
                }
                "datagrams_receive_received" => {
                    participant.audio_metrics.datagrams_received = row.value as u64;
                    participant.video_metrics.datagrams_received = row.value as u64;
                }
                "datagrams_receive_packet_loss_rate" => {
                    participant.audio_metrics.packet_loss_rate = row.value;
                    participant.video_metrics.packet_loss_rate = row.value;
                }
                _ => {}
            }
        }
    }

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

    /// Format duration in human readable format
    fn format_duration(&self, seconds: Option<i64>) -> String {
        match seconds {
            Some(secs) => {
                if secs >= 3600 {
                    format!("{:.1}h", secs as f64 / 3600.0)
                } else if secs >= 60 {
                    format!("{:.1}m", secs as f64 / 60.0)
                } else {
                    format!("{}s", secs)
                }
            }
            None => "Active".to_string(),
        }
    }
}

impl Default for ParticipantAudioVideoMetrics {
    fn default() -> Self {
        Self {
            decoder_output_count: 0,
            decoder_decode_count: 0,
            scheduler_schedule_count: 0,
            restore_commit_count: 0,
            preprocessing_gap_duration_avg: 0.0,
            gain_normalization_factor: 0.0,
            source_volume: 0.0,
            streams_active: 0,
            datagrams_expected: 0,
            datagrams_lost: 0,
            datagrams_received: 0,
            packet_loss_rate: 0.0,
        }
    }
}

impl Collector for ParticipantCollector {
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
                .ok_or_else(|| eyre::eyre!("Space ID is required for participant-level metrics"))?;

            // Get participant timeline
            let timeline = self
                .query_participant_timeline(space_id, start_time, duration_seconds)
                .await?;
            let mut participants = self.process_participant_timeline(&timeline);

            // For each participant, collect detailed metrics
            for participant in &mut participants {
                // Collect CPU usage
                let cpu_data = self
                    .query_participant_cpu_usage(space_id, participant.participant_id, start_time, duration_seconds)
                    .await?;
                self.process_cpu_usage(participant, &cpu_data);

                // Collect throughput
                let throughput_data = self
                    .query_participant_throughput(space_id, participant.participant_id, start_time, duration_seconds)
                    .await?;
                self.process_throughput(participant, &throughput_data);

                // Collect latency
                let latency_data = self
                    .query_participant_latency(space_id, participant.participant_id, start_time, duration_seconds)
                    .await?;
                self.process_latency(participant, &latency_data);

                // Collect audio/video metrics
                let audio_video_data = self
                    .query_participant_audio_video_metrics(
                        space_id,
                        participant.participant_id,
                        start_time,
                        duration_seconds,
                    )
                    .await?;
                self.process_audio_video_metrics(participant, &audio_video_data);
            }

            // Store metrics internally
            self.metrics = Some(participants);
            Ok(())
        })
    }

    fn format(&self) -> String {
        let participants = match &self.metrics {
            Some(p) => p,
            None => return "No participant metrics collected yet. Call collect() first.".to_string(),
        };

        if participants.is_empty() {
            return "No participants found in the specified time range.".to_string();
        }

        let mut output = String::new();

        // Main participants overview table
        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new(format!("👥 PARTICIPANT METRICS ({} participants)", participants.len()))
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Cyan),
                Cell::new("Latency").add_attribute(Attribute::Bold),
                Cell::new("Bytes Sent").add_attribute(Attribute::Bold),
                Cell::new("Bytes Received").add_attribute(Attribute::Bold),
            ]);

        // Add participant rows
        for participant in participants {
            table.add_row(vec![
                Cell::new(format!("Participant {}", participant.participant_id)).add_attribute(Attribute::Bold),
                Cell::new(format!("{:.1}ms", participant.avg_latency_ms))
                    .fg(self.get_latency_color(participant.avg_latency_ms)),
                Cell::new(self.format_bytes(participant.total_bytes_sent)),
                Cell::new(self.format_bytes(participant.total_bytes_received)),
            ]);
        }

        output.push_str(&format!("{}\n", table));

        // Detailed metrics for each participant
        for participant in participants {
            output.push_str(&self.format_participant_details(participant));
        }

        output
    }

    fn summary(&self) -> serde_json::Value {
        let participants = match &self.metrics {
            Some(p) => p,
            None => return serde_json::json!({"error": "No participant metrics collected yet"}),
        };

        serde_json::json!({
            "participants": participants.iter().map(|p| {
                serde_json::json!({
                    "participant_id": p.participant_id,
                    "space_id": p.space_id,
                    "server_url": p.server_url,
                    "timestamp": p.timestamp,
                    "join_time": p.join_time,
                    "leave_time": p.leave_time,
                    "duration_seconds": p.duration_seconds,
                    "status": if p.leave_time.is_some() { "left" } else { "active" },
                    "cpu": {
                        "avg_usage": p.avg_cpu_usage,
                        "max_usage": p.max_cpu_usage
                    },
                    "network": {
                        "bytes_sent": p.total_bytes_sent,
                        "bytes_received": p.total_bytes_received,
                        "packets_sent": p.total_packets_sent,
                        "packets_received": p.total_packets_received
                    },
                    "latency": {
                        "avg_ms": p.avg_latency_ms,
                        "max_ms": p.max_latency_ms,
                        "p95_ms": p.p95_latency_ms
                    },
                    "audio_metrics": {
                        "decoder_output_count": p.audio_metrics.decoder_output_count,
                        "decoder_decode_count": p.audio_metrics.decoder_decode_count,
                        "scheduler_schedule_count": p.audio_metrics.scheduler_schedule_count,
                        "preprocessing_gap_duration_avg": p.audio_metrics.preprocessing_gap_duration_avg,
                        "gain_normalization_factor": p.audio_metrics.gain_normalization_factor,
                        "source_volume": p.audio_metrics.source_volume,
                        "streams_active": p.audio_metrics.streams_active,
                        "datagrams_expected": p.audio_metrics.datagrams_expected,
                        "datagrams_lost": p.audio_metrics.datagrams_lost,
                        "datagrams_received": p.audio_metrics.datagrams_received,
                        "packet_loss_rate": p.audio_metrics.packet_loss_rate
                    },
                    "video_metrics": {
                        "decoder_output_count": p.video_metrics.decoder_output_count,
                        "decoder_decode_count": p.video_metrics.decoder_decode_count,
                        "scheduler_schedule_count": p.video_metrics.scheduler_schedule_count,
                        "restore_commit_count": p.video_metrics.restore_commit_count,
                        "preprocessing_gap_duration_avg": p.video_metrics.preprocessing_gap_duration_avg,
                        "gain_normalization_factor": p.video_metrics.gain_normalization_factor,
                        "source_volume": p.video_metrics.source_volume,
                        "streams_active": p.video_metrics.streams_active,
                        "datagrams_expected": p.video_metrics.datagrams_expected,
                        "datagrams_lost": p.video_metrics.datagrams_lost,
                        "datagrams_received": p.video_metrics.datagrams_received,
                        "packet_loss_rate": p.video_metrics.packet_loss_rate
                    }
                })
            }).collect::<Vec<_>>()
        })
    }

    fn name(&self) -> &'static str {
        "ParticipantCollector"
    }
}

impl ParticipantCollector {
    /// Format detailed metrics for a single participant
    fn format_participant_details(&self, participant: &ParticipantData) -> String {
        let mut output = String::new();

        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![Cell::new(format!(
                "📊 Participant {} Details",
                participant.participant_id
            ))
            .add_attribute(Attribute::Bold)
            .fg(Color::Blue)]);

        // Timeline information
        table.add_row(vec![
            Cell::new("Join Time").add_attribute(Attribute::Bold),
            Cell::new(participant.join_time.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        ]);

        if let Some(leave_time) = participant.leave_time {
            table.add_row(vec![
                Cell::new("Leave Time").add_attribute(Attribute::Bold),
                Cell::new(leave_time.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
            ]);
        }

        table.add_row(vec![
            Cell::new("Duration").add_attribute(Attribute::Bold),
            Cell::new(self.format_duration(participant.duration_seconds)),
        ]);

        // Performance metrics
        table.add_row(vec![
            Cell::new("Avg CPU Usage").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}%", participant.avg_cpu_usage)).fg(self.get_cpu_color(participant.avg_cpu_usage)),
        ]);

        table.add_row(vec![
            Cell::new("Max CPU Usage").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}%", participant.max_cpu_usage)).fg(self.get_cpu_color(participant.max_cpu_usage)),
        ]);

        // Network metrics
        table.add_row(vec![
            Cell::new("Bytes Sent").add_attribute(Attribute::Bold),
            Cell::new(self.format_bytes(participant.total_bytes_sent)),
        ]);

        table.add_row(vec![
            Cell::new("Bytes Received").add_attribute(Attribute::Bold),
            Cell::new(self.format_bytes(participant.total_bytes_received)),
        ]);

        table.add_row(vec![
            Cell::new("Packets Sent").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.total_packets_sent)),
        ]);

        table.add_row(vec![
            Cell::new("Packets Received").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.total_packets_received)),
        ]);

        // Latency metrics
        table.add_row(vec![
            Cell::new("Avg Latency").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}ms", participant.avg_latency_ms))
                .fg(self.get_latency_color(participant.avg_latency_ms)),
        ]);

        table.add_row(vec![
            Cell::new("Max Latency").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}ms", participant.max_latency_ms))
                .fg(self.get_latency_color(participant.max_latency_ms)),
        ]);

        table.add_row(vec![
            Cell::new("P95 Latency").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.1}ms", participant.p95_latency_ms))
                .fg(self.get_latency_color(participant.p95_latency_ms)),
        ]);

        output.push_str(&format!("{}\n", table));

        // Audio/Video metrics
        output.push_str(&self.format_audio_video_metrics(participant));

        output
    }

    /// Format audio/video metrics for a participant
    fn format_audio_video_metrics(&self, participant: &ParticipantData) -> String {
        let mut output = String::new();

        // Audio metrics table
        let mut audio_table = Table::new();
        audio_table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![Cell::new(format!(
                "🎵 Audio Metrics - Participant {}",
                participant.participant_id
            ))
            .add_attribute(Attribute::Bold)
            .fg(Color::Magenta)]);

        audio_table.add_row(vec![
            Cell::new("Decoder Output").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.audio_metrics.decoder_output_count)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Decoder Decode").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.audio_metrics.decoder_decode_count)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Scheduler Schedule").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.audio_metrics.scheduler_schedule_count)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Gap Duration Avg").add_attribute(Attribute::Bold),
            Cell::new(format!(
                "{:.2}ms",
                participant.audio_metrics.preprocessing_gap_duration_avg
            )),
        ]);

        audio_table.add_row(vec![
            Cell::new("Gain Normalization").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.3}", participant.audio_metrics.gain_normalization_factor)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Source Volume").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.3}", participant.audio_metrics.source_volume)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Streams Active").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", participant.audio_metrics.streams_active)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Datagrams Expected").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.audio_metrics.datagrams_expected)),
        ]);

        audio_table.add_row(vec![
            Cell::new("Datagrams Lost").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", participant.audio_metrics.datagrams_lost)).fg(
                if participant.audio_metrics.datagrams_lost == 0 {
                    Color::Green
                } else {
                    Color::Red
                },
            ),
        ]);

        audio_table.add_row(vec![
            Cell::new("Packet Loss Rate").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.2}%", participant.audio_metrics.packet_loss_rate)).fg(
                if participant.audio_metrics.packet_loss_rate < 1.0 {
                    Color::Green
                } else {
                    Color::Yellow
                },
            ),
        ]);

        output.push_str(&format!("{}\n", audio_table));

        // Video metrics table
        let mut video_table = Table::new();
        video_table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![Cell::new(format!(
                "🎬 Video Metrics - Participant {}",
                participant.participant_id
            ))
            .add_attribute(Attribute::Bold)
            .fg(Color::Magenta)]);

        video_table.add_row(vec![
            Cell::new("Decoder Output").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.video_metrics.decoder_output_count)),
        ]);

        video_table.add_row(vec![
            Cell::new("Restore Commit").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.video_metrics.restore_commit_count)),
        ]);

        video_table.add_row(vec![
            Cell::new("Streams Active").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", participant.video_metrics.streams_active)),
        ]);

        video_table.add_row(vec![
            Cell::new("Datagrams Expected").add_attribute(Attribute::Bold),
            Cell::new(self.format_number(participant.video_metrics.datagrams_expected)),
        ]);

        video_table.add_row(vec![
            Cell::new("Datagrams Lost").add_attribute(Attribute::Bold),
            Cell::new(format!("{}", participant.video_metrics.datagrams_lost)).fg(
                if participant.video_metrics.datagrams_lost == 0 {
                    Color::Green
                } else {
                    Color::Red
                },
            ),
        ]);

        video_table.add_row(vec![
            Cell::new("Packet Loss Rate").add_attribute(Attribute::Bold),
            Cell::new(format!("{:.2}%", participant.video_metrics.packet_loss_rate)).fg(
                if participant.video_metrics.packet_loss_rate < 1.0 {
                    Color::Green
                } else {
                    Color::Yellow
                },
            ),
        ]);

        output.push_str(&format!("{}\n", video_table));

        output
    }
}

// ClickHouse row structs
#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct ParticipantTimelineRow {
    participant_id: u16,
    first_seen: i64, // milliseconds since epoch
    last_seen: i64,  // milliseconds since epoch
    #[allow(dead_code)]
    data_points: u64,
}

#[derive(Debug, Clone, serde::Deserialize, clickhouse::Row)]
struct CpuUsageRow {
    #[allow(dead_code)]
    space_id: String,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    receiving_participant_id: u16,
    #[allow(dead_code)]
    sending_participant_id: u16,
    #[allow(dead_code)]
    media_type: u8, // 0 = audio, 1 = video
    #[allow(dead_code)]
    stream_id: u8,
    #[allow(dead_code)]
    ts: i64, // milliseconds since epoch
    #[allow(dead_code)]
    collect: u16,
    #[allow(dead_code)]
    encode: u16,
    #[allow(dead_code)]
    send: u16,
    #[allow(dead_code)]
    sender: u16,
    #[allow(dead_code)]
    relay: u16,
    #[allow(dead_code)]
    receiver: u16,
    #[allow(dead_code)]
    decode: u16,
    total: u16,
}

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
