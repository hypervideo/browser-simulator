use chrono::{
    DateTime,
    Utc,
};
use serde::{
    Deserialize,
    Serialize,
};

/// Space-level data (CPU, memory, network, latency per participant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceData {
    pub space_id: String,
    pub server_url: String,
    pub timestamp: DateTime<Utc>,

    // Space-wide CPU metrics
    pub total_cpu_usage_percent: f64,
    pub avg_cpu_usage_percent: f64,
    pub max_cpu_usage_percent: f64,

    // Space-wide network metrics
    pub total_network_bytes_sent: u64,
    pub total_network_bytes_received: u64,
    pub total_network_errors: u64,

    // Latency metrics per participant
    pub participant_latencies: Vec<ParticipantLatencyMetrics>,

    // Participant count
    pub participant_count: u32,

    // Audio/Video processing metrics
    pub audio_processing_metrics: AudioVideoProcessingMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioVideoProcessingMetrics {
    // Audio processing metrics
    pub audio_decoder_output_count: u64,
    pub audio_decoder_decode_count: u64,
    pub audio_scheduler_schedule_count: u64,
    pub audio_preprocessing_gap_duration_avg: f64,
    pub audio_gain_normalization_factor: f64,
    pub audio_source_volume: f64,

    // Video processing metrics
    pub video_decoder_output_count: u64,
    pub video_restore_commit_count: u64,
    pub video_streams_active: u32,
    pub video_send_key_data_request_count: u64,

    // Network metrics
    pub datagrams_receive_expected: u64,
    pub datagrams_receive_lost: u64,
    pub datagrams_receive_received: u64,
    pub datagrams_receive_packet_loss_rate: f64,
}

impl Default for AudioVideoProcessingMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioVideoProcessingMetrics {
    pub fn new() -> Self {
        Self {
            audio_decoder_output_count: 0,
            audio_decoder_decode_count: 0,
            audio_scheduler_schedule_count: 0,
            audio_preprocessing_gap_duration_avg: 0.0,
            audio_gain_normalization_factor: 0.0,
            audio_source_volume: 0.0,
            video_decoder_output_count: 0,
            video_restore_commit_count: 0,
            video_streams_active: 0,
            video_send_key_data_request_count: 0,
            datagrams_receive_expected: 0,
            datagrams_receive_lost: 0,
            datagrams_receive_received: 0,
            datagrams_receive_packet_loss_rate: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantLatencyMetrics {
    pub participant_id: u16,
    pub sending_participant_id: u16,
    pub media_type: String, // "audio" or "video"
    pub stream_id: u8,
    pub timestamp: DateTime<Utc>,

    // Latency breakdown (in milliseconds)
    pub collect_latency: u16,
    pub encode_latency: u16,
    pub send_latency: u16,
    pub sender_latency: u16,
    pub relay_latency: u16,
    pub receiver_latency: u16,
    pub decode_latency: u16,
    pub total_latency: u16,

    // Throughput metrics
    pub throughput_bps: f64,
    pub packets_per_second: f64,
}

impl SpaceData {
    pub fn new(space_id: String, server_url: String) -> Self {
        Self {
            space_id,
            server_url,
            timestamp: Utc::now(),
            total_cpu_usage_percent: 0.0,
            avg_cpu_usage_percent: 0.0,
            max_cpu_usage_percent: 0.0,
            total_network_bytes_sent: 0,
            total_network_bytes_received: 0,
            total_network_errors: 0,
            participant_latencies: Vec::new(),
            participant_count: 0,
            audio_processing_metrics: AudioVideoProcessingMetrics::new(),
        }
    }

    pub fn total_network_mbps_sent(&self, interval_seconds: f64) -> f64 {
        if interval_seconds <= 0.0 {
            return 0.0;
        }
        (self.total_network_bytes_sent as f64 * 8.0) / interval_seconds / 1_000_000.0
    }

    pub fn total_network_mbps_received(&self, interval_seconds: f64) -> f64 {
        if interval_seconds <= 0.0 {
            return 0.0;
        }
        (self.total_network_bytes_received as f64 * 8.0) / interval_seconds / 1_000_000.0
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.participant_latencies.is_empty() {
            return 0.0;
        }
        self.participant_latencies
            .iter()
            .map(|p| p.total_latency as f64)
            .sum::<f64>()
            / self.participant_latencies.len() as f64
    }

    pub fn p95_latency_ms(&self) -> f64 {
        if self.participant_latencies.is_empty() {
            return 0.0;
        }
        let mut latencies: Vec<f64> = self
            .participant_latencies
            .iter()
            .map(|p| p.total_latency as f64)
            .collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95_index = (latencies.len() as f64 * 0.95) as usize;
        latencies[p95_index.min(latencies.len() - 1)]
    }

    pub fn max_latency_ms(&self) -> f64 {
        self.participant_latencies
            .iter()
            .map(|p| p.total_latency as f64)
            .fold(0.0, f64::max)
    }
}

impl ParticipantLatencyMetrics {
    pub fn new(participant_id: u16, sending_participant_id: u16, media_type: String, stream_id: u8) -> Self {
        Self {
            participant_id,
            sending_participant_id,
            media_type,
            stream_id,
            timestamp: Utc::now(),
            collect_latency: 0,
            encode_latency: 0,
            send_latency: 0,
            sender_latency: 0,
            relay_latency: 0,
            receiver_latency: 0,
            decode_latency: 0,
            total_latency: 0,
            throughput_bps: 0.0,
            packets_per_second: 0.0,
        }
    }
}
