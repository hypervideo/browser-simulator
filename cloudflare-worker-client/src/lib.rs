mod client;
pub mod generated;

pub use client::{
    CloudflareWorkerClient,
    DEPLOYED_WORKER_URL,
    LOCAL_WORKER_URL,
};
pub use generated::types;
