//! # Collectors Module
//!
//! This module contains the core data collection logic for the stats gatherer.
//!
//! ## Architecture
//!
//! - **`Collector` trait**: Defines the interface for all metric collectors
//! - **`ServerCollector`**: Collects server-level metrics (health, CPU, memory, network)
//! - **`SpaceCollector`**: Collects space-level metrics (participants, CPU, network, latency, audio/video processing)
//! - **`Orchestrator`**: Coordinates all collectors and manages the overall data collection flow
//!
//! ## Data Sources
//!
//! - **ClickHouse**: Primary data source for all metrics
//! - **HTTP Health Checks**: Server health and response time
//!
//! ## Key Features
//!
//! - **Modular design**: Each collector handles specific metric types
//! - **Error handling**: Comprehensive error handling with detailed logging
//! - **Performance optimized**: Efficient queries and data processing
//! - **Extensible**: Easy to add new metric types and collectors

pub mod collector;
pub mod orchestrator;
pub mod participant_collector;
pub mod server_collector;
pub mod space_collector;

// Re-export the main types for easy access
pub use collector::Collector;
pub use orchestrator::Orchestrator;
pub use participant_collector::ParticipantCollector;
pub use server_collector::ServerCollector;
pub use space_collector::SpaceCollector;
