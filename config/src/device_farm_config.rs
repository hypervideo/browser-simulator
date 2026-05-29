use serde::{
    Deserialize,
    Serialize,
};

const DEFAULT_REGION: &str = "us-west-2";

/// Configuration for the AWS Device Farm desktop-browser ("Test Grid") backend.
///
/// Credentials are NOT stored here; they are resolved from the standard AWS
/// credential chain (env vars / shared profile) by `aws-config` at runtime.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceFarmConfig {
    /// ARN of the Device Farm Test Grid project (from Terraform output).
    pub project_arn: String,
    /// AWS region; Device Farm Test Grid is only available in us-west-2.
    pub region: String,
    /// Lifetime requested for the generated Selenium connection URL.
    pub url_expires_seconds: u64,
    /// `aws:maxDurationSecs` capability - hard cap on total session length.
    pub session_max_duration_ms: u64,
    /// `aws:idleTimeoutSecs` capability - max gap between WebDriver commands.
    pub idle_timeout_ms: u64,
    pub navigation_timeout_ms: u64,
    pub selector_timeout_ms: u64,
    /// How often the keep-alive poller pings the live session.
    pub health_poll_interval_ms: u64,
    pub debug: bool,
}

impl DeviceFarmConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl Default for DeviceFarmConfig {
    fn default() -> Self {
        Self {
            project_arn: String::new(),
            region: DEFAULT_REGION.to_owned(),
            url_expires_seconds: 300,
            session_max_duration_ms: 1_800_000,
            idle_timeout_ms: 180_000,
            navigation_timeout_ms: 45_000,
            selector_timeout_ms: 20_000,
            health_poll_interval_ms: 30_000,
            debug: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_targets_us_west_2_and_is_default() {
        let config = DeviceFarmConfig::default();
        assert_eq!(config.region, "us-west-2");
        assert!(config.is_default());
        // Poll interval must stay safely inside the idle timeout window.
        assert!(config.health_poll_interval_ms < config.idle_timeout_ms);
    }
}
