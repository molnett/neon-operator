use actix_web::{middleware, web::Data, App, HttpServer};
use anyhow::Result;

use crate::handlers::{compute, health, hooks, metrics};

/// Configure and start the HTTP server
pub async fn start_server(
    cluster_state: neon_cluster::controllers::cluster_controller::State,
) -> Result<()> {
    let server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(cluster_state.clone()))
            .wrap(middleware::Logger::default().exclude("/health"))
            .service(health::index)
            .service(health::health)
            .service(metrics::metrics)
            .service(hooks::notify_attach)
            .service(compute::compute_spec)
    })
    .bind("0.0.0.0:8080")?
    .shutdown_timeout(5);

    server.run().await?;
    Ok(())
}