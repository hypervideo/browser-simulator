use serde::{
    Deserialize,
    Serialize,
};
use std::env;

pub const DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV: &str = "DEVICE_FARM_AWS_ACCESS_KEY_ID";
pub const DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV: &str = "DEVICE_FARM_AWS_SECRET_ACCESS_KEY";
pub const DEVICE_FARM_AWS_REGION_ENV: &str = "DEVICE_FARM_AWS_REGION";
pub const DEVICE_FARM_PROJECT_ARN_ENV: &str = "DEVICE_FARM_PROJECT_ARN";
pub const DEVICE_FARM_AWS_PROFILE: &str = "hyper-client-simulator";
const DEFAULT_DEVICE_FARM_AWS_REGION: &str = "us-west-2";
const DEFAULT_DEVICE_FARM_PROJECT_ARN: &str =
    "arn:aws:devicefarm:us-west-2:891377399831:testgrid-project:4bdbcfcd-2c2b-432d-ac13-d71cb9586c5f";

pub fn device_farm_aws_access_key_id() -> Result<String, env::VarError> {
    env::var(DEVICE_FARM_AWS_ACCESS_KEY_ID_ENV)
}

pub fn device_farm_aws_secret_access_key() -> Result<String, env::VarError> {
    env::var(DEVICE_FARM_AWS_SECRET_ACCESS_KEY_ENV)
}

pub fn default_device_farm_region() -> String {
    env::var(DEVICE_FARM_AWS_REGION_ENV).unwrap_or_else(|_| DEFAULT_DEVICE_FARM_AWS_REGION.to_owned())
}

pub fn default_device_farm_project_arn() -> String {
    env::var(DEVICE_FARM_PROJECT_ARN_ENV).unwrap_or_else(|_| DEFAULT_DEVICE_FARM_PROJECT_ARN.to_owned())
}

/// Configuration for the AWS Device Farm desktop-browser ("Test Grid") backend.
///
/// Credentials are read from the process environment when the AWS client is
/// created.
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
            project_arn: default_device_farm_project_arn(),
            region: default_device_farm_region(),
            url_expires_seconds: 300,
            session_max_duration_ms: 1_800_000,
            idle_timeout_ms: 180_000,
            health_poll_interval_ms: 30_000,
            debug: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_runtime_env_fallbacks_and_is_default() {
        let config = DeviceFarmConfig::default();
        assert_eq!(config.region, default_device_farm_region());
        assert_eq!(config.project_arn, default_device_farm_project_arn());
        assert!(config.is_default());
        // Poll interval must stay safely inside the idle timeout window.
        assert!(config.health_poll_interval_ms < config.idle_timeout_ms);
    }
}
