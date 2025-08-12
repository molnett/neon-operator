use axum::{extract::State, http::StatusCode, response::Json, routing::post, Router};
use axum_server::tls_rustls::RustlsConfig;
use kube::Client;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

mod validator;
mod cert_reloader;

use validator::PageserverValidator;
use cert_reloader::CertificateReloader;

#[derive(Clone)]
struct AppState {
    validator: PageserverValidator,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::CryptoProvider::install_default(rustls::crypto::aws_lc_rs::default_provider()).ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    loop {
        info!("Starting NeonPageserver admission webhook");

        let client = Client::try_default().await?;
        let validator = PageserverValidator::new(client.clone());

        let state = AppState { validator };

        let app = Router::new()
            .route("/validate", post(validate_handler))
            .layer(TraceLayer::new_for_http())
            .with_state(Arc::new(state));

        // Health check server (HTTP) - only start once
        static HEALTH_STARTED: std::sync::Once = std::sync::Once::new();
        HEALTH_STARTED.call_once(|| {
            tokio::spawn(async {
                let health_app = Router::new().route("/health", axum::routing::get(health_handler));
                let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
                info!("Health server listening on 0.0.0.0:8080");
                axum::serve(listener, health_app).await.unwrap();
            });
        });

        // Start certificate watcher
        let cert_reloader = CertificateReloader::new();
        cert_reloader.start_watching("/etc/certs").await?;

        // Load TLS configuration
        let tls_config = load_tls_config().await?;

        info!("Admission webhook listening on 0.0.0.0:8443 (HTTPS) with certificate auto-reload");
        
        // Start server in background
        let server_handle = tokio::spawn(async move {
            axum_server::bind_rustls("0.0.0.0:8443".parse().unwrap(), tls_config)
                .serve(app.into_make_service())
                .await
        });

        // Check for certificate changes every second
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            
            if cert_reloader.should_restart() {
                info!("Certificate change detected - restarting server");
                server_handle.abort();
                break;
            }
            
            if server_handle.is_finished() {
                match server_handle.await {
                    Ok(Ok(())) => {
                        info!("Server exited normally");
                        return Ok(());
                    }
                    Ok(Err(e)) => {
                        warn!("Server error: {}", e);
                        break;
                    }
                    Err(_) => {
                        info!("Server aborted for restart");
                        break;
                    }
                }
            }
        }
        
        // Small delay before restart
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

async fn load_tls_config() -> anyhow::Result<RustlsConfig> {
    let config = RustlsConfig::from_pem_file("/etc/certs/tls.crt", "/etc/certs/tls.key").await?;

    Ok(config)
}

async fn validate_handler(
    State(state): State<Arc<AppState>>,
    Json(review): Json<
        kube::core::admission::AdmissionReview<neon_cluster::api::v1alpha1::neonpageserver::NeonPageserver>,
    >,
) -> Result<
    Json<kube::core::admission::AdmissionReview<neon_cluster::api::v1alpha1::neonpageserver::NeonPageserver>>,
    StatusCode,
> {
    info!("Received admission review request");
    match state.validator.validate_pageserver(review).await {
        Ok(response_review) => {
            info!("Validation successful, sending response");
            Ok(Json(response_review))
        },
        Err(e) => {
            warn!("Validation error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn health_handler() -> &'static str {
    "healthy"
}
