use actix_web::{get, web::Data, HttpRequest, HttpResponse, Responder};

#[get("/health")]
pub async fn health(_: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("healthy")
}

#[get("/")]
pub async fn index(
    c: Data<neon_cluster::controllers::cluster_controller::State>,
    _req: HttpRequest,
) -> impl Responder {
    let d = c.diagnostics().await;
    HttpResponse::Ok().json(&d)
}
