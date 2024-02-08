use std::fmt;
use thiserror::Error;
use tokio::time::Duration;

#[derive(Error, Debug)]
pub enum StandardError {
    #[error("SerializationError: {0}")]
    SerializationError(#[source] serde_json::Error),

    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    // NB: awkward type because finalizer::Error embeds the reconciler error (which is this)
    // so boxing this error to break cycles
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("IllegalDocument")]
    IllegalDocument,
}

impl StandardError {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

#[derive(Error, Debug)]
pub struct ErrorWithRequeue {
    duration: Duration,
    #[from]
    #[source]
    error: StandardError,
}

impl ErrorWithRequeue {
    fn new(error: StandardError, duration: Duration) -> ErrorWithRequeue {
        ErrorWithRequeue {
            error: error,
            duration: duration,
        }
    }
}

impl fmt::Display for ErrorWithRequeue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Standard Error: {0}")]
    StandardError(#[source] StandardError),

    #[error("Error With Requeue: {0}")]
    ErrorWithRequeue(#[source] ErrorWithRequeue),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Expose all controller components used by main
pub mod controller;
pub use crate::controller::*;

/// Log and trace integrations
pub mod telemetry;

/// Metrics
mod metrics;
pub use metrics::Metrics;

/// NeonStorage
mod neon_storage;
pub use neon_storage::NeonStorage;

/// StorageBroker
mod storage_broker;
pub use storage_broker::reconcile;

#[cfg(test)]
pub mod fixtures;
