//! # Hyper.Video Stats Gatherer
//!
//! A comprehensive analytics tool for collecting and displaying metrics from Hyper.Video sessions.
//!
//! ## Features
//!
//! - **Server-level metrics**: Health status, CPU usage, memory usage, network traffic
//! - **Space-level metrics**: Participant count, CPU usage, network traffic, latency metrics
//! - **Audio/Video processing metrics**: Decoder performance, scheduler metrics, gap analysis
//! - **Network quality metrics**: Packet loss, datagram statistics
//! - **Beautiful terminal display**: Enhanced tables with color-coded performance indicators
//! - **JSON export**: Programmatic access to collected data
//!
//! ## Architecture
//!
//! The tool is built with a modular architecture where each collector is self-contained:
//!
//! - **`config`**: Configuration management and CLI argument parsing
//! - **`metrics`**: Data structures for server and space-level metrics
//! - **`collectors`**: Self-contained data collection and formatting modules
//!   - **`ServerCollector`**: Collects server-level metrics and handles display/JSON export
//!   - **`SpaceCollector`**: Collects space-level metrics and handles display/JSON export
//!   - **`Orchestrator`**: Creates shared clients and coordinates all collectors
//!
//! ## Usage
//!
//! ```bash
//! # Collect metrics for a specific space
//! stats-gatherer --clickhouse-url=http://clickhouse:8123 \
//!                --clickhouse-user=default \
//!                --space-url=https://server.com/SPACE-ID \
//!                --duration=5m
//!
//! # Export to JSON file
//! stats-gatherer --output-file=metrics.json \
//!                --clickhouse-url=http://clickhouse:8123 \
//!                --clickhouse-user=default \
//!                --space-url=https://server.com/SPACE-ID \
//!                --duration=5m
//! ```

pub mod collectors;
pub mod config;
pub mod metrics;

pub use collectors::*;
pub use config::Config;
pub use metrics::*;
