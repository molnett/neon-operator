use std::error;
use std::fmt;
use thiserror::Error;
use tokio::time::Duration;

#[derive(Error, Debug)]
pub enum StdError {
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

impl StdError {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

#[derive(Error, Debug)]
pub struct ErrorWithRequeue {
    pub duration: Duration,
    pub error: StdError,
}

impl ErrorWithRequeue {
    pub fn new(error: StdError, duration: Duration) -> ErrorWithRequeue {
        ErrorWithRequeue {
            error: error,
            duration: duration,
        }
    }

    pub fn metric_label(&self) -> String {
        self.error.metric_label()
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
    StdError(#[source] StdError),

    #[error("Error With Requeue: {0}")]
    ErrorWithRequeue(#[source] ErrorWithRequeue),
}

impl Error {
    pub fn metric_label(&self) -> String {
        self.metric_label()
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
