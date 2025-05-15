use super::resources::{NeonBranch, NeonProject};
use crate::util::branch_status::{
    BranchPhase, BranchStatusManager
};
use crate::util::errors::{Error, Result, StdError};

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    ConfigMap, Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::api::Api;
use kube::Resource;
use serde_json::json;
use tracing::info;

pub async fn ensure_config_map(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    branch: &NeonBranch,
    project: &NeonProject,
) -> Result<()> {
    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let config_map_name = format!("{}-compute-spec", name);

    if config_maps.get(&config_map_name).await.is_err() {
        let mut config_map = create_compute_spec_config_map(name, branch, project);

        // Set owner reference using controller_owner_ref
        config_map.metadata.owner_references =
            branch.controller_owner_ref(&()).map(|owner_ref| vec![owner_ref]);

        config_maps
            .create(&Default::default(), &config_map)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        info!("Created ConfigMap: {}", config_map_name);
        return Ok(());
    }

    info!("ConfigMap already exists: {}", config_map_name);

    Ok(())
}

pub async fn ensure_deployment(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    branch: &NeonBranch,
) -> Result<()> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let deployment_name = format!("{}-compute-node", name);

    if deployments.get(&deployment_name).await.is_err() {
        let mut deployment = create_compute_node_deployment(name, branch);

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
    let phase = if compute_node_ready { BranchPhase::Ready } else { BranchPhase::Pending };
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

pub fn create_compute_spec_config_map(name: &str, branch: &NeonBranch, project: &NeonProject) -> ConfigMap {
    let project_id = project.spec.id.clone();
    let project_name = project.spec.name.clone();
    let cluster_name = project.spec.cluster_name.clone();

    let spec_json = json!({
        "spec": {
            "format_version": 1.0,
            "cluster": {
                "cluster_id": project_id,
                "name": project_name,
                "roles": [
                    {
                        "name": project.spec.superuser_name,
                        "encrypted_password": "b093c0d3b281ba6da1eacc608620abd8",
                        "options": null
                    }
                ],
                "databases": [],
                "settings": [
                    {"name": "fsync", "value": "off", "vartype": "bool"},
                    {"name": "wal_level", "value": "logical", "vartype": "enum"},
                    {"name": "wal_log_hints", "value": "on", "vartype": "bool"},
                    {"name": "log_connections", "value": "on", "vartype": "bool"},
                    {"name": "port", "value": "55433", "vartype": "integer"},
                    {"name": "shared_buffers", "value": "1MB", "vartype": "string"},
                    {"name": "max_connections", "value": "100", "vartype": "integer"},
                    {"name": "listen_addresses", "value": "0.0.0.0", "vartype": "string"},
                    {"name": "max_wal_senders", "value": "10", "vartype": "integer"},
                    {"name": "max_replication_slots", "value": "10", "vartype": "integer"},
                    {"name": "wal_sender_timeout", "value": "5s", "vartype": "string"},
                    {"name": "wal_keep_size", "value": "0", "vartype": "integer"},
                    {"name": "password_encryption", "value": "md5", "vartype": "enum"},
                    {"name": "restart_after_crash", "value": "off", "vartype": "bool"},
                    {"name": "synchronous_standby_names", "value": "walproposer", "vartype": "string"},
                    {"name": "shared_preload_libraries", "value": "neon", "vartype": "string"},
                    {"name": "neon.safekeepers", "value": format!(
                        "safekeeper-{0}-0.neon.svc.cluster.local:5454,safekeeper-{0}-1.neon.svc.cluster.local:5454,safekeeper-{0}-2.neon.svc.cluster.local:5454",
                        cluster_name
                    ), "vartype": "string"},
                    {"name": "neon.timeline_id", "value": branch.spec.timeline_id.clone().unwrap_or_default(), "vartype": "string"},
                    {"name": "neon.tenant_id", "value": project.spec.tenant_id, "vartype": "string"},
                    {"name": "neon.pageserver_connstring", "value": format!("host=pageserver-{0}.neon.svc.cluster.local port=6400", cluster_name), "vartype": "string"},
                    {"name": "max_replication_write_lag", "value": "500MB", "vartype": "string"},
                    {"name": "max_replication_flush_lag", "value": "10GB", "vartype": "string"}
                ]
            },
            "delta_operations": [],
            "safekeeper_connstrings": [
                format!("postgresql://postgres:@safekeeper-{0}-0.neon.svc.cluster.local:5454", cluster_name),
                format!("postgresql://postgres:@safekeeper-{0}-1.neon.svc.cluster.local:5454", cluster_name),
                format!("postgresql://postgres:@safekeeper-{0}-2.neon.svc.cluster.local:5454", cluster_name)
            ]
        },
        "compute_ctl_config": {
            "jwks": {
                "keys": [
                    {
                        "use": "sig",
                        "key_ops": [
                            "verify"
                        ],
                        "alg": "EdDSA",
                        "kid": "ZGIxMzAzOGY0YWQwODk2ODU1MTk1NzMxMDFkYmUyOWU2NzZkOWNjNjMyMGRkZGJjOWY0MjdjYWVmNzE1MjUyOAo=",
                        "kty": "OKP",
                        "crv": "Ed25519",
                        "x": "MGQ4ZDFhOTdmNTM0NmUwMDc3ZmJkN2Q0MWE0ZmI3M2NhNWE3YjFjOTNkM2IyYzRkZTQzOGM3MjBkZTk3N2E5ZAo="
                    }
                ]
            }
        }
    });

    ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{}-compute-spec", name)),
            ..Default::default()
        },
        data: Some(
            [(
                "spec.json".to_string(),
                serde_json::to_string_pretty(&spec_json).unwrap(),
            )]
            .into_iter()
            .collect(),
        ),
        ..Default::default()
    }
}

pub fn create_compute_node_deployment(name: &str, branch: &NeonBranch) -> Deployment {
    let deployment_name = format!("{}-compute-node", name);
    let labels = std::collections::BTreeMap::from([("app".to_string(), deployment_name.clone())]);

    Deployment {
        metadata: ObjectMeta {
            name: Some(deployment_name.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "storage-broker".to_string(),
                        image: Some(format!("neondatabase/compute-node-v{}", branch.spec.pg_version)),
                        args: Some(vec![
                            "--pgdata".to_string(),
                            "/.neon/data/pgdata".to_string(),
                            "--connstr=postgresql://cloud_admin:@0.0.0.0:55433/postgres".to_string(),
                            "--compute-id".to_string(),
                            name.to_string(),
                            "--config".to_string(),
                            "/var/spec.json".to_string(),
                            "--pgbin".to_string(),
                            "/usr/local/bin/postgres".to_string(),
                        ]),
                        ports: Some(vec![ContainerPort {
                            container_port: 55433,
                            ..Default::default()
                        }]),
                        volume_mounts: Some(vec![
                            VolumeMount {
                                name: "spec-volume".to_string(),
                                mount_path: "/var".to_string(),
                                ..Default::default()
                            },
                            VolumeMount {
                                name: "pgdata".to_string(),
                                mount_path: "/.neon/data".to_string(),
                                ..Default::default()
                            },
                        ]),
                        env: Some(vec![EnvVar {
                            name: "OTEL_SDK_DISABLED".to_string(),
                            value: Some("true".to_string()),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![
                        Volume {
                            name: "spec-volume".to_string(),
                            config_map: Some(k8s_openapi::api::core::v1::ConfigMapVolumeSource {
                                name: Some(format!("{}-compute-spec", name)),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        Volume {
                            name: "pgdata".to_string(),
                            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource {
                                size_limit: Some(Quantity("500Mi".to_string())),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        status: None,
    }
}
