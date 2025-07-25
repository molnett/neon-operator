use actix_web::{get, web::Data, HttpRequest, HttpResponse, Responder};
use prometheus::{Encoder, TextEncoder};

#[get("/metrics")]
pub async fn metrics(
    c: Data<neon_cluster::controllers::cluster_controller::State>,
    _req: HttpRequest,
) -> impl Responder {
    let metrics = c.metrics();
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&metrics, &mut buffer).unwrap();
    HttpResponse::Ok().body(buffer)
}
