use serde::{
    Deserialize,
    Serialize,
};

const DEFAULT_BASE_URL: &str = "https://cloudflare-browser-simulator.hyper-video.workers.dev";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudflareConfig {
    pub base_url: url::Url,
    pub request_timeout_seconds: u64,
    pub session_timeout_ms: u64,
    pub navigation_timeout_ms: u64,
    pub selector_timeout_ms: u64,
    pub debug: bool,
    pub health_poll_interval_ms: u64,
}

impl CloudflareConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl Default for CloudflareConfig {
    fn default() -> Self {
        Self {
            base_url: url::Url::parse(DEFAULT_BASE_URL).expect("valid Cloudflare worker base URL"),
            request_timeout_seconds: 180,
            session_timeout_ms: 600_000,
            navigation_timeout_ms: 45_000,
            selector_timeout_ms: 20_000,
            debug: false,
            health_poll_interval_ms: 5_000,
        }
    }
}
