use crate::controllers::resources::{NeonBranch, NeonProject};
use crate::storage_controller::client::StorageControllerClient;
use crate::util::errors::{Error, Result, StdError};
use crate::util::secrets::get_jwt_keys_from_secret;
use k8s_openapi::api::apps::v1::Deployment;
use kube::api::{Api, ListParams};
use kube::ResourceExt;
use serde_json::json;
use tracing::{error, info};

pub async fn generate_compute_spec(client: &kube::Client, compute_id: &str) -> Result<serde_json::Value> {
    info!("Starting compute spec generation for compute_id: {}", compute_id);

    // 1. Find the compute deployment to get cluster context
    let deployment = match find_compute_deployment(client, compute_id).await {
        Ok(d) => d,
        Err(e) => {
            error!(
                "Failed to find compute deployment for compute_id {}: {}",
                compute_id, e
            );
            return Err(e);
        }
    };

    let cluster_name = match extract_cluster_name(&deployment) {
        Ok(name) => name,
        Err(e) => {
            error!("Failed to extract cluster name from deployment: {}", e);
            return Err(e);
        }
    };

    info!("Found cluster name: {}", cluster_name);

    let tenant_id = match deployment.labels().get("neon.tenant_id") {
        Some(id) => id,
        None => {
            error!("Deployment missing neon.tenant_id label");
            return Err(Error::StdError(StdError::MetadataMissing(
                "neon.tenant_id label not found".into(),
            )));
        }
    };

    let timeline_id = match deployment.labels().get("neon.timeline_id") {
        Some(id) => id,
        None => {
            error!("Deployment missing neon.timeline_id label");
            return Err(Error::StdError(StdError::MetadataMissing(
                "neon.timeline_id label not found".into(),
            )));
        }
    };

    info!("Found tenant_id: {}, timeline_id: {}", tenant_id, timeline_id);

    // 2. Get project and branch details
    let (project, branch) = match find_project_and_branch(client, &tenant_id, &timeline_id).await {
        Ok(result) => result,
        Err(e) => {
            error!(
                "Failed to find project and branch for tenant_id: {}, timeline_id: {}: {}",
                tenant_id, timeline_id, e
            );
            return Err(e);
        }
    };

    // 3. Get JWT keys from cluster secret
    let jwks = match get_jwt_keys_from_secret(client, &cluster_name).await {
        Ok(keys) => keys,
        Err(e) => {
            error!(
                "Failed to get JWT keys from secret for cluster {}: {}",
                cluster_name, e
            );
            return Err(e);
        }
    };

    info!("Successfully retrieved JWT keys");

    // 4. Get pageserver connection string - fetch from storage-controller
    let storage_client = StorageControllerClient::new(cluster_name.as_str());
    let pageserver_connstring = match storage_client.get_pageserver_connstring(tenant_id).await {
        Ok(connstring) => {
            info!("Retrieved pageserver connection string: {}", connstring);
            connstring
        }
        Err(e) => {
            error!(
                "Failed to get pageserver connection string for tenant {}: {}",
                tenant_id, e
            );
            return Err(e);
        }
    };

    // 5. Construct safekeeper connections (always 3)
    let safekeeper_connstrings: Vec<String> = (0..3)
        .map(|i| {
            format!(
                "postgresql://postgres:@safekeeper-{}-{}.neon:5454",
                cluster_name, i
            )
        })
        .collect();

    // 6. Build postgres settings
    let settings = build_postgres_settings(
        &cluster_name,
        &project.spec.tenant_id.unwrap(),
        &branch.spec.timeline_id.unwrap(),
        &pageserver_connstring,
        Some(2048),
    );

    // 7. Generate spec
    Ok(json!({
        "spec": {
            "format_version": 1.0,
            "suspend_timeout_seconds": -1,
            "cluster": {
                "cluster_id": project.spec.id,
                "name": project.spec.name,
                "roles": [{
                    "name": project.spec.superuser_name,
                    "encrypted_password": "b093c0d3b281ba6da1eacc608620abd8",
                    "options": null
                }],
                "databases": [],
                "settings": settings,
            },
            "delta_operations": [],
            "safekeeper_connstrings": safekeeper_connstrings,
        },
        "compute_ctl_config": {
            "jwks": jwks
        },
        "status": "attached"
    }))
}

async fn find_compute_deployment(client: &kube::Client, compute_id: &str) -> Result<Deployment> {
    let deployments: Api<Deployment> = Api::all(client.clone());
    let deployment_name = format!("{}-compute-node", compute_id);

    info!("Looking for deployment: {}", deployment_name);

    let deps = deployments
        .list(&ListParams {
            label_selector: Some(format!("app={}", deployment_name)),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            error!("Failed to get deployment {}: {}", deployment_name, e);
            Error::StdError(StdError::KubeError(e))
        })?;

    Ok(deps.clone().iter().next().unwrap().clone())
}

fn extract_cluster_name(deployment: &Deployment) -> Result<String> {
    deployment
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get("neon.cluster_name"))
        .cloned()
        .ok_or_else(|| {
            Error::StdError(StdError::MetadataMissing(
                "neon.cluster_name annotation not found".into(),
            ))
        })
}

async fn find_project_and_branch(
    client: &kube::Client,
    tenant_id: &str,
    timeline_id: &str,
) -> Result<(NeonProject, NeonBranch)> {
    info!(
        "Searching for project with tenant_id: {} and branch with timeline_id: {}",
        tenant_id, timeline_id
    );

    // Find project by tenant_id
    let projects: Api<NeonProject> = Api::all(client.clone());
    let project_list = projects.list(&Default::default()).await.map_err(|e| {
        error!("Failed to list NeonProject resources: {}", e);
        Error::StdError(StdError::KubeError(e))
    })?;

    let project = project_list
        .items
        .into_iter()
        .find(|p| p.spec.tenant_id.as_ref() == Some(&tenant_id.to_string()))
        .ok_or_else(|| {
            error!("No NeonProject found with tenant_id: {}", tenant_id);
            Error::StdError(StdError::MetadataMissing(format!(
                "Project with tenant_id {} not found",
                tenant_id
            )))
        })?;

    info!(
        "Found project: {}",
        project.metadata.name.as_ref().unwrap_or(&"<unnamed>".to_string())
    );

    // Find branch by timeline_id
    let branches: Api<NeonBranch> = Api::all(client.clone());
    let branch_list = branches.list(&Default::default()).await.map_err(|e| {
        error!("Failed to list NeonBranch resources: {}", e);
        Error::StdError(StdError::KubeError(e))
    })?;

    let branch = branch_list
        .items
        .into_iter()
        .find(|b| b.spec.timeline_id.as_ref() == Some(&timeline_id.to_string()))
        .ok_or_else(|| {
            error!("No NeonBranch found with timeline_id: {}", timeline_id);
            Error::StdError(StdError::MetadataMissing(format!(
                "Branch with timeline_id {} not found",
                timeline_id
            )))
        })?;

    info!(
        "Found branch: {}",
        branch.metadata.name.as_ref().unwrap_or(&"<unnamed>".to_string())
    );

    Ok((project, branch))
}

fn build_postgres_settings(
    cluster_name: &str,
    tenant_id: &str,
    timeline_id: &str,
    pageserver_connstring: &str,
    shard_stripe_size: Option<u32>,
) -> Vec<serde_json::Value> {
    let mut settings = vec![
        json!({"name": "fsync", "value": "off", "vartype": "bool"}),
        json!({"name": "wal_level", "value": "logical", "vartype": "enum"}),
        json!({"name": "wal_log_hints", "value": "on", "vartype": "bool"}),
        json!({"name": "log_connections", "value": "on", "vartype": "bool"}),
        json!({"name": "port", "value": "55433", "vartype": "integer"}),
        json!({"name": "shared_buffers", "value": "1MB", "vartype": "string"}),
        json!({"name": "max_connections", "value": "100", "vartype": "integer"}),
        json!({"name": "listen_addresses", "value": "0.0.0.0", "vartype": "string"}),
        json!({"name": "max_wal_senders", "value": "10", "vartype": "integer"}),
        json!({"name": "max_replication_slots", "value": "10", "vartype": "integer"}),
        json!({"name": "wal_sender_timeout", "value": "5s", "vartype": "string"}),
        json!({"name": "wal_keep_size", "value": "0", "vartype": "integer"}),
        json!({"name": "password_encryption", "value": "md5", "vartype": "enum"}),
        json!({"name": "restart_after_crash", "value": "off", "vartype": "bool"}),
        json!({"name": "synchronous_standby_names", "value": "walproposer", "vartype": "string"}),
        json!({"name": "shared_preload_libraries", "value": "neon", "vartype": "string"}),
        json!({
            "name": "neon.safekeepers",
            "value": format!(
                "safekeeper-{0}-0.neon:5454,safekeeper-{0}-1.neon:5454,safekeeper-{0}-2.neon:5454",
                cluster_name
            ),
            "vartype": "string"
        }),
        json!({"name": "neon.timeline_id", "value": timeline_id, "vartype": "string"}),
        json!({"name": "neon.tenant_id", "value": tenant_id, "vartype": "string"}),
        json!({"name": "neon.pageserver_connstring", "value": pageserver_connstring, "vartype": "string"}),
        json!({"name": "max_replication_write_lag", "value": "500MB", "vartype": "string"}),
        json!({"name": "max_replication_flush_lag", "value": "10GB", "vartype": "string"}),
    ];

    // Add shard_stripe_size if provided
    if let Some(stripe_size) = shard_stripe_size {
        settings.push(json!({
            "name": "neon.shard_stripe_size",
            "value": stripe_size.to_string(),
            "vartype": "integer"
        }));
    }

    settings
}
