use actix_web::{put, web::Json, HttpResponse, Responder};
use kube::Client;
use neon_cluster::compute::spec::ComputeHookNotifyRequest;
use tracing::{error, warn};

use crate::services::hook_service::HookService;

#[put("/notify-attach")]
pub async fn notify_attach(req_body: Json<ComputeHookNotifyRequest>) -> impl Responder {
    let client = match Client::try_default().await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create kube client: {}", e);
            return HttpResponse::InternalServerError().json(format!("Failed to create kube client: {}", e));
        }
    };

    let hook_service = HookService::new(client);

    match hook_service.process_notify_attach(&req_body).await {
        Ok(message) => HttpResponse::Ok().json(message),
        Err(e) => {
            if e.contains("No compute pods matching tenant ID") {
                warn!("No compute pods matching tenant ID");
                HttpResponse::NotFound().json(e)
            } else {
                error!("Failed to notify attach: {}", e);
                HttpResponse::InternalServerError().json(e)
            }
        }
    }
}
