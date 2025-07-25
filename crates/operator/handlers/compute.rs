use actix_web::{get, HttpResponse, Responder};
use kube::Client;
use tracing::error;

use crate::services::compute_service::ComputeService;

#[get("/compute/api/v2/computes/{compute_id}/spec")]
pub async fn compute_spec(compute_id: actix_web::web::Path<String>) -> impl Responder {
    let client = match Client::try_default().await {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create kube client: {}", e);
            return HttpResponse::InternalServerError().json(format!("Failed to create kube client: {}", e));
        }
    };

    let compute_service = ComputeService::new(client);

    match compute_service.generate_spec(compute_id.as_str()).await {
        Ok(spec) => HttpResponse::Ok().json(spec),
        Err(e) => {
            error!("Failed to generate compute spec: {}", e);
            HttpResponse::InternalServerError().json(format!("Failed to generate compute spec: {}", e))
        }
    }
}
