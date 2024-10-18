#![allow(unused_imports, unused_variables)]
use actix_web::{get, middleware, web::Data, App, HttpRequest, HttpResponse, HttpServer, Responder};

mod neon_cluster;
use neon_cluster::controller;

mod util;
use util::telemetry;

use prometheus::{Encoder, TextEncoder};

#[get("/metrics")]
async fn metrics(c: Data<neon_cluster::controller::State>, _req: HttpRequest) -> impl Responder {
    let metrics = c.metrics();
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&metrics, &mut buffer).unwrap();
    HttpResponse::Ok().body(buffer)
}

#[get("/health")]
async fn health(_: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("healthy")
}

#[get("/")]
async fn index(c: Data<neon_cluster::controller::State>, _req: HttpRequest) -> impl Responder {
    let d = c.diagnostics().await;
    HttpResponse::Ok().json(&d)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init().await;

    // Initiatilize Kubernetes controller state
    let state = neon_cluster::controller::State::default();
    let neon_cluster_controller = neon_cluster::controller::run(state.clone());

    // Start web server
    let server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(state.clone()))
            .wrap(middleware::Logger::default().exclude("/health"))
            .service(index)
            .service(health)
            .service(metrics)
    })
    .bind("0.0.0.0:8080")?
    .shutdown_timeout(5);

    // Both runtimes implements graceful shutdown, so poll until both are done
    tokio::join!(neon_cluster_controller, server.run()).1?;
    Ok(())
}
