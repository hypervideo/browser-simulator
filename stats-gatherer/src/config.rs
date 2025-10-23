//! # Configuration Module
//!
//! This module handles configuration management and CLI argument parsing for the stats gatherer.
//!
//! ## Key Components
//!
//! - **`Config`**: Main configuration structure containing all settings
//!
//! ## Configuration Fields
//!
//! - **ClickHouse connection**: URL, username, password
//! - **Space URL**: Target space for metrics collection
//! - **Collection settings**: Interval duration
//! - **Output settings**: Optional JSON export file path
//! - **Parsed fields**: Server URL and space ID extracted from space URL
//!
//! ## URL Parsing
//!
//! The module automatically extracts server URL and space ID from the provided space URL.
//! Expected format: `https://server.com/m/SPACE_ID` or `https://server.com/SPACE_ID`

use eyre::Result;
use serde::{
    Deserialize,
    Serialize,
};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub clickhouse_url: String,
    pub clickhouse_user: String,
    pub clickhouse_password: Option<String>,
    pub space_url: String,
    pub collection_interval: Duration,
    pub output_file: Option<String>,
    // Parsed from space_url
    pub server_url: String,
    pub space_id: Option<String>,
}

impl Config {
    pub fn new(
        clickhouse_url: String,
        clickhouse_user: String,
        clickhouse_password: Option<String>,
        space_url: String,
        collection_interval: Duration,
        output_file: Option<String>,
    ) -> Result<Self> {
        let space_id = Self::parse_space_id(&space_url)?;
        let server_url = Self::extract_server_url(&space_url)?;

        Ok(Self {
            clickhouse_url,
            clickhouse_user,
            clickhouse_password,
            space_url,
            collection_interval,
            output_file,
            server_url,
            space_id,
        })
    }

    /// Parse space URL to extract space ID
    /// Expected format: `https://server.com/m/SPACE_ID` or `https://server.com/SPACE_ID/`
    fn parse_space_id(space_url: &str) -> Result<Option<String>> {
        let url = url::Url::parse(space_url)?;

        // Extract space ID from path segments
        let path_segments: Vec<&str> = url
            .path_segments()
            .map(|segments| segments.collect())
            .unwrap_or_default();

        let space_id = match path_segments.as_slice() {
            ["m", id, ..] if !id.is_empty() => Some((*id).to_string()),
            [id, ..] if !id.is_empty() && *id != "m" => Some((*id).to_string()),
            _ => None,
        };

        Ok(space_id)
    }

    /// Extract server URL from space URL
    /// Expected format: `https://server.com/m/SPACE_ID` or `https://server.com/SPACE_ID/`
    fn extract_server_url(space_url: &str) -> Result<String> {
        let url = url::Url::parse(space_url)?;
        let host = url
            .host_str()
            .ok_or_else(|| eyre::eyre!("No host found in space URL: {}", space_url))?;
        let server_url = match url.port() {
            Some(port) => format!("{}://{}:{}", url.scheme(), host, port),
            None => format!("{}://{}", url.scheme(), host),
        };
        Ok(server_url)
    }
}
