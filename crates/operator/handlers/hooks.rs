use actix_web::{put, web::Json, HttpResponse, Responder};
use kube::Client;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::services::hook_service::{ComputeHookNotifyRequestShard, HookService};

/// Request body that we send to the control plane to notify it of where a tenant is attached
#[derive(Serialize, Deserialize, Debug)]
pub struct ComputeHookNotifyRequest {
    pub tenant_id: String,
    pub stripe_size: Option<u32>,
    pub shards: Vec<ComputeHookNotifyRequestShard>,
}

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

    match hook_service
        .process_notify_attach(&req_body.tenant_id, req_body.stripe_size, &req_body.shards)
        .await
    {
        Ok(message) => HttpResponse::Ok().json(message),
        Err(e) => {
            if e.contains("No compute pods matching tenant ID") {
                HttpResponse::NotFound().json(e)
            } else {
                HttpResponse::InternalServerError().json(e)
            }
        }
    }
}
