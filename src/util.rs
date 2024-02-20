pub(crate) mod errors;
pub(crate) mod metrics;
pub(crate) mod telemetry;

pub use errors::*;

pub type Result<T, E = Error> = std::result::Result<T, E>;
