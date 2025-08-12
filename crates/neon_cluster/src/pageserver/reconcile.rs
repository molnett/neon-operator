use crate::api::v1alpha1::neonpageserver::NeonPageserver;
use crate::pageserver::deployment::reconcile_pageserver_deployment;
use crate::util::errors::Result;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

use kube::runtime::controller::Action;
use kube::Resource;
use kube::ResourceExt;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::info;

use crate::controllers::pageserver_controller::Context;

use super::{config, service};

pub async fn reconcile(neon_pageserver: &NeonPageserver, ctx: Arc<Context>) -> Result<Action> {
    let ns = neon_pageserver.namespace().unwrap_or_default();
    info!(
        "Reconciling Pageserver '{}' in namespace '{}'",
        neon_pageserver.metadata.name.as_ref().unwrap(),
        ns
    );

    let oref = neon_pageserver
        .controller_owner_ref(&())
        .unwrap_or_else(|| OwnerReference {
            api_version: "oltp.molnett.org/v1alpha1".to_string(),
            kind: "NeonPageserver".to_string(),
            controller: Some(true),
            name: neon_pageserver.metadata.name.clone().unwrap(),
            uid: format!("deployment-{}", neon_pageserver.metadata.name.clone().unwrap()),
            ..Default::default()
        });

    let computed_name = format!(
        "{}-pageserver-{}",
        neon_pageserver.spec.cluster, neon_pageserver.spec.id
    );

    config::reconcile_configmap(
        &ctx.client,
        &ns,
        &computed_name,
        &neon_pageserver.spec.cluster,
        &neon_pageserver.spec.bucket_credentials_secret,
        &oref,
    )
    .await?;

    let pageserver_id = neon_pageserver.spec.id.to_string();

    // Reconcile pageserver deployment
    reconcile_pageserver_deployment(
        &ctx.client,
        &computed_name,
        &ns,
        neon_pageserver,
        &pageserver_id.clone(),
        &neon_pageserver.spec.bucket_credentials_secret,
        &oref,
    )
    .await?;

    // Create service for pageserver deployment

    service::reconcile_pageserver_service(&ctx.client, &computed_name, &ns, &pageserver_id, &oref).await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}
