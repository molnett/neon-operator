use tracing_subscriber::{prelude::*, EnvFilter, Registry};

/// Initialize tracing
pub async fn init() {
    // Setup tracing layers
    let logger = tracing_subscriber::fmt::layer().compact();
    let env_filter = EnvFilter::try_from_default_env()
        .or(EnvFilter::try_new("info"))
        .unwrap();

    let collector = Registry::default().with(logger).with(env_filter);

    // Initialize tracing
    tracing::subscriber::set_global_default(collector).unwrap();
}
