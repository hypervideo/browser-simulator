pub mod server_data;
pub mod shared;
pub mod space_data;

// Re-export the main types for easy access
use chrono::{
    DateTime,
    Utc,
};
use serde::{
    Deserialize,
    Serialize,
};
pub use server_data::*;
pub use shared::*;
pub use space_data::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedData {
    // Current data
    pub server_level_data: Option<ServerData>,
    pub space_level_data: Option<SpaceData>,
    pub collection_start: DateTime<Utc>,
    pub collection_end: DateTime<Utc>,
    pub collection_duration_seconds: f64,
}

impl CollectedData {
    pub fn new(collection_start: DateTime<Utc>) -> Self {
        Self {
            server_level_data: None,
            space_level_data: None,
            collection_start,
            collection_end: collection_start,
            collection_duration_seconds: 0.0,
        }
    }

    pub fn finalize(&mut self) {
        self.collection_end = Utc::now();
        // Don't override collection_duration_seconds as it's set to the actual time window being queried
    }
}
