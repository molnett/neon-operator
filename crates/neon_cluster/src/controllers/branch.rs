use super::resources::{NeonBranch, NeonProject};
use crate::compute;
use crate::util::branch_status::{BranchPhase, BranchStatusManager};
use crate::util::errors::{Error, Result, StdError};
use crate::util::secrets::get_jwt_keys_from_secret;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::Api;
use kube::Resource;
use serde_json::json;
use tracing::info;

pub async fn ensure_minimal_config_map(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    branch: &NeonBranch,
    project: &NeonProject,
) -> Result<()> {
    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let config_map_name = format!("{}-compute-spec", name);

    // Get JWT keys from the cluster's secret
    let jwks = get_jwt_keys_from_secret(client, &project.spec.cluster_name).await?;

    // Create minimal config with only compute_ctl_config
    let config_data = json!({
        "compute_ctl_config": {
            "jwks": jwks
        }
    });

    let desired_config_map = ConfigMap {
        metadata: ObjectMeta {
            name: Some(config_map_name.clone()),
            owner_references: branch.controller_owner_ref(&()).map(|owner_ref| vec![owner_ref]),
            ..Default::default()
        },
        data: Some(
            [(
                "spec.json".to_string(),
                serde_json::to_string_pretty(&config_data)
                    .map_err(|e| Error::StdError(StdError::JsonSerializationError(e)))?,
            )]
            .into_iter()
            .collect(),
        ),
        ..Default::default()
    };

    // Check if ConfigMap already exists
    match config_maps.get(&config_map_name).await {
        Ok(_) => {
            info!("ConfigMap '{}' already exists", config_map_name);
            Ok(())
        }
        Err(kube::Error::Api(err)) if err.code == 404 => {
            // ConfigMap doesn't exist, create it
            info!("Creating minimal ConfigMap '{}' with JWT keys", config_map_name);
            config_maps
                .create(&Default::default(), &desired_config_map)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            Ok(())
        }
        Err(e) => Err(Error::StdError(StdError::KubeError(e))),
    }
}

pub async fn ensure_deployment(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    branch: &NeonBranch,
    project: &NeonProject,
) -> Result<()> {
    // First ensure minimal ConfigMap exists for JWT keys
    ensure_minimal_config_map(client, namespace, name, branch, project).await?;

    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment_name = format!("{}-compute-node", name);

    if deployments.get(&deployment_name).await.is_err() {
        let mut deployment = compute::create_compute_deployment(name, branch, project);

        // Set owner reference using controller_owner_ref
        deployment.metadata.owner_references =
            branch.controller_owner_ref(&()).map(|owner_ref| vec![owner_ref]);

        deployments
            .create(&Default::default(), &deployment)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        info!("Created Deployment: {}", deployment_name);
        return Ok(());
    }

    info!("Deployment already exists: {}", deployment_name);

    Ok(())
}

pub async fn is_compute_node_ready(client: &kube::Client, namespace: &str, name: &str) -> Result<bool> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment_name = format!("{}-compute-node", name);

    let deployment = deployments
        .get(&deployment_name)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
    let status = deployment.status.unwrap_or_default();
    let ready_replicas = status.ready_replicas.unwrap_or(0);
    let replicas = status.replicas.unwrap_or(0);

    Ok(ready_replicas == replicas && replicas > 0)
}

pub async fn update_status(
    client: &kube::Client,
    _namespace: &str,
    _name: &str,
    branch: &NeonBranch,
    compute_node_ready: bool,
) -> Result<()> {
    let status_manager = BranchStatusManager::new(client, branch)?;

    // Update compute node readiness condition
    status_manager.set_compute_node_ready(compute_node_ready).await?;

    // Update phase based on compute node readiness
    let phase = if compute_node_ready {
        BranchPhase::Ready
    } else {
        BranchPhase::Pending
    };
    status_manager.update_phase(phase).await?;

    Ok(())
}

pub async fn get_or_create_default_user(
    client: &kube::Client,
    _namespace: &str,
    _name: &str,
    branch: &NeonBranch,
) -> Result<()> {
    // TODO: Implement logic to create default user in the Compute node

    // Update status to indicate default user has been created
    let status_manager = BranchStatusManager::new(client, branch)?;
    status_manager.set_default_user_created().await?;

    Ok(())
}

pub async fn create_default_database(
    client: &kube::Client,
    _namespace: &str,
    _name: &str,
    branch: &NeonBranch,
) -> Result<()> {
    // TODO: Implement logic to create default database in the Compute node

    // Update status to indicate default database has been created
    let status_manager = BranchStatusManager::new(client, branch)?;
    status_manager.set_default_database_created().await?;

    Ok(())
}
