use chrono::{
    DateTime,
    Utc,
};
use serde::{
    Deserialize,
    Serialize,
};

/// CPU data point with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuDataPoint {
    pub timestamp: DateTime<Utc>,
    pub cpu_usage_percent: f64,
}

/// Participant join event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantJoinEvent {
    pub participant_id: u16,
    pub first_seen: DateTime<Utc>,
}

/// Server-level data (CPU, memory, network)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerData {
    pub server_url: String,
    pub timestamp: DateTime<Utc>,

    // CPU metrics
    pub cpu_usage_percent: f64,
    pub cpu_load_average: f64,
    pub cpu_data_points: Vec<CpuDataPoint>,

    // Participant join events
    pub participant_join_events: Vec<ParticipantJoinEvent>,

    // Memory metrics
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_usage_percent: f64,

    // Network metrics
    pub network_bytes_sent: u64,
    pub network_bytes_received: u64,
    pub network_packets_sent: u64,
    pub network_packets_received: u64,
    pub network_errors: u64,

    // Server health
    pub is_healthy: bool,
    pub response_time_ms: f64,
    pub status_code: Option<u16>,
}

impl ServerData {
    pub fn new(server_url: String) -> Self {
        Self {
            server_url,
            timestamp: Utc::now(),
            cpu_usage_percent: 0.0,
            cpu_load_average: 0.0,
            cpu_data_points: Vec::new(),
            participant_join_events: Vec::new(),
            memory_used_bytes: 0,
            memory_total_bytes: 0,
            memory_usage_percent: 0.0,
            network_bytes_sent: 0,
            network_bytes_received: 0,
            network_packets_sent: 0,
            network_packets_received: 0,
            network_errors: 0,
            is_healthy: false,
            response_time_ms: 0.0,
            status_code: None,
        }
    }

    pub fn memory_usage_gb(&self) -> f64 {
        self.memory_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn memory_total_gb(&self) -> f64 {
        self.memory_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn network_mbps_sent(&self, seconds: f64) -> f64 {
        if seconds <= 0.0 {
            return 0.0;
        }
        (self.network_bytes_sent as f64 * 8.0) / (1_000_000.0 * seconds)
    }

    pub fn network_mbps_received(&self, seconds: f64) -> f64 {
        if seconds <= 0.0 {
            return 0.0;
        }
        (self.network_bytes_received as f64 * 8.0) / (1_000_000.0 * seconds)
    }
}
