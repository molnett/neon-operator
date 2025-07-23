use crate::util::errors::{Error, ErrorWithRequeue, Result, StdError};

use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

use kube::runtime::controller::Action;
use kube::Resource;
use kube::{api::ListParams, Api, Client, ResourceExt};

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::info;

use super::super::cluster_controller::Context;
use super::super::resources::*;
use super::{config, pod, service};

pub async fn reconcile(neon_cluster: &NeonCluster, ctx: Arc<Context>) -> Result<Action> {
    let cluster_name = match &neon_cluster.metadata.name {
        Some(name) => name,
        None => {
            return Err(Error::ErrorWithRequeue(ErrorWithRequeue::new(
                StdError::MetadataMissing("Name should always be set on an existing object".to_string()),
                Duration::from_secs(5 * 60),
            )));
        }
    };
    let name = format!("pageserver-{}", cluster_name);

    let ns = neon_cluster.namespace().unwrap_or_default();
    info!("Reconciling Pageserver '{}' in namespace '{}'", name, ns);

    let oref = neon_cluster
        .controller_owner_ref(&())
        .unwrap_or_else(|| OwnerReference {
            api_version: "oltp.molnett.org/v1".to_string(),
            kind: "NeonCluster".to_string(),
            controller: Some(true),
            name: neon_cluster.metadata.name.clone().unwrap(),
            uid: format!("deployment-{}", neon_cluster.metadata.name.clone().unwrap()),
            ..Default::default()
        });

    config::reconcile_configmap(
        &ctx.client,
        &ns,
        &name,
        &neon_cluster.name_any(),
        &neon_cluster.spec.bucket_credentials_secret,
        &oref,
    )
    .await?;

    // Reconcile individual pageserver pods instead of StatefulSet
    reconcile_pageserver_pods(
        &ctx.client,
        &ns,
        cluster_name,
        &neon_cluster.spec.bucket_credentials_secret,
        neon_cluster.spec.num_pageservers,
        &oref,
    )
    .await?;

    // Create headless service for pod discovery
    let headless_service_name = format!("{}-headless", name);
    service::reconcile_headless_service(&ctx.client, &ns, &headless_service_name, cluster_name, &oref)
        .await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn reconcile_pageserver_pods(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    bucket_credentials_secret: &str,
    num_pageservers: i32,
    oref: &OwnerReference,
) -> Result<()> {
    // Get existing pageserver pods
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/name".to_string(),
        format!("pageserver-{}", cluster_name),
    );
    labels.insert(
        "app.kubernetes.io/component".to_string(),
        "pageserver".to_string(),
    );

    let lp = ListParams::default().labels(&format!(
        "app.kubernetes.io/name=pageserver-{},app.kubernetes.io/component=pageserver",
        cluster_name
    ));

    let existing_pods = pods
        .list(&lp)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    // Extract existing pageserver IDs
    let mut existing_ids: Vec<String> = existing_pods
        .items
        .iter()
        .filter_map(|pod| {
            pod.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("neon.io/pageserver-id"))
                .cloned()
        })
        .collect();

    // Create new pods if needed
    let current_count = existing_ids.len() as i32;
    if current_count < num_pageservers {
        for _ in current_count..num_pageservers {
            let new_id = pod::generate_unique_pageserver_id(&existing_ids);
            pod::reconcile_single_pageserver_pod(
                client,
                namespace,
                cluster_name,
                &new_id,
                bucket_credentials_secret,
                oref,
            )
            .await?;
            existing_ids.push(new_id);
        }
    }

    // Handle pod deletions with finalizers
    for pod in &existing_pods.items {
        if pod.metadata.deletion_timestamp.is_some() {
            pod::handle_pageserver_deletion(pod, client).await?;
        }
    }

    Ok(())
}
