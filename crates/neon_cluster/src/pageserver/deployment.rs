use crate::api::v1alpha1::neonpageserver::NeonPageserver;
use crate::controllers::pageserver_controller::FIELD_MANAGER;
use crate::util::errors::{Error, ErrorWithRequeue, Result, StdError};

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, ContainerPort, EnvVar, EnvVarSource, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, PodSecurityContext, PodSpec, PodTemplateSpec, SecretKeySelector, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};

use kube::ResourceExt;
use kube::{
    api::{Patch, PatchParams},
    Api, Client,
};

use serde_json::json;
use std::collections::BTreeMap;
use tokio::time::Duration;
use tracing::{info, warn};

const PAGESERVER_FINALIZER: &str = "neon.io/drain-required";

pub async fn reconcile_pageserver_deployment(
    client: &Client,
    name: &str,
    namespace: &str,
    neon_pageserver: &NeonPageserver,
    pageserver_id: &str,
    bucket_credentials_secret: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);

    // Ensure PVC exists first
    create_pageserver_pvc(client, name, namespace, neon_pageserver, pageserver_id, oref).await?;

    // Create labels for the intended deployment
    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), name.to_string());
    labels.insert(
        "app.kubernetes.io/component".to_string(),
        "pageserver".to_string(),
    );
    labels.insert("neon.io/pageserver-id".to_string(), pageserver_id.to_string());

    // Create the intended deployment spec
    let intended_deployment = create_pageserver_deployment_spec(
        namespace,
        &name,
        neon_pageserver,
        pageserver_id,
        bucket_credentials_secret,
        labels,
        oref,
    );

    // Apply the deployment using server-side apply
    // SSA will no-op if the deployment is already in the desired state
    // Use force() to take ownership from previous field managers or "unknown"
    let patch_params = PatchParams::apply(FIELD_MANAGER).force();
    deployments
        .patch(name, &patch_params, &Patch::Apply(&intended_deployment))
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    info!("Deployment '{}' reconciled via server-side apply", name);
    Ok(())
}

async fn create_pageserver_pvc(
    client: &Client,
    name: &str,
    namespace: &str,
    neon_pageserver: &NeonPageserver,
    pageserver_id: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);

    // Check if PVC already exists
    match pvcs.get(&name).await {
        Ok(_) => {
            info!("PVC '{}' already exists", name);
            return Ok(());
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            // Create PVC
            let mut labels = BTreeMap::new();
            labels.insert(
                "app.kubernetes.io/name".to_string(),
                format!(
                    "pageserver-{}",
                    neon_pageserver
                        .metadata
                        .name
                        .clone()
                        .expect("Kubernetes object without name in Metadata")
                ),
            );
            labels.insert(
                "app.kubernetes.io/component".to_string(),
                "pageserver".to_string(),
            );
            labels.insert("neon.io/pageserver-id".to_string(), pageserver_id.to_string());

            let pvc = PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    namespace: Some(namespace.to_string()),
                    labels: Some(labels),
                    owner_references: Some(vec![oref.clone()]),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    storage_class_name: neon_pageserver.spec.storage_config.storage_class.clone(),
                    resources: Some(k8s_openapi::api::core::v1::VolumeResourceRequirements {
                        requests: Some({
                            let mut map = BTreeMap::new();
                            map.insert(
                                "storage".to_string(),
                                Quantity(neon_pageserver.spec.storage_config.size.clone()),
                            );
                            map
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            };

            info!("Creating PVC '{}'", name);
            let patch_params = PatchParams::apply(FIELD_MANAGER).force();
            pvcs.patch(name, &patch_params, &Patch::Apply(&pvc))
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

pub async fn handle_pageserver_deletion(deployment: &Deployment, client: &Client) -> Result<()> {
    let deployment_name = deployment.metadata.name.as_ref().unwrap();
    let namespace = deployment.metadata.namespace.as_ref().unwrap();

    // Check if deployment has our finalizer
    if let Some(finalizers) = &deployment.metadata.finalizers {
        if !finalizers.contains(&PAGESERVER_FINALIZER.to_string()) {
            return Ok(());
        }
    } else {
        return Ok(());
    }

    info!(
        "Handling deletion for pageserver deployment '{}'",
        deployment_name
    );

    // Check if pageserver is drained
    if !is_pageserver_drained(deployment).await? {
        warn!(
            "Pageserver '{}' is not drained, triggering drain",
            deployment_name
        );
        trigger_pageserver_drain(deployment).await?;
        // Return error to requeue
        return Err(Error::ErrorWithRequeue(ErrorWithRequeue::new(
            StdError::InvalidArgument("Waiting for pageserver drain".to_string()),
            Duration::from_secs(30),
        )));
    }

    // Remove finalizer
    info!("Pageserver '{}' is drained, removing finalizer", deployment_name);
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let finalizers: Vec<String> = deployment
        .finalizers()
        .iter()
        .filter(|f| *f != PAGESERVER_FINALIZER)
        .cloned()
        .collect();

    let patch = json!({
        "metadata": {
            "finalizers": finalizers
        }
    });

    deployments
        .patch(deployment_name, &PatchParams::default(), &Patch::Merge(patch))
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    Ok(())
}

async fn is_pageserver_drained(deployment: &Deployment) -> Result<bool> {
    // TODO: Implement actual drain check via storage-controller API (#21)
    // For now, we'll check if a specific annotation is set
    if let Some(annotations) = &deployment.metadata.annotations {
        return Ok(annotations.get("neon.io/drained") == Some(&"true".to_string()));
    }
    Ok(false)
}

async fn trigger_pageserver_drain(deployment: &Deployment) -> Result<()> {
    // TODO: Implement actual drain trigger via storage-controller API (#21)
    // For now, we'll just log
    let deployment_name = deployment.metadata.name.as_ref().unwrap();
    let pageserver_id = deployment
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get("neon.io/pageserver-id"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    info!(
        "Triggering drain for pageserver '{}' with ID '{}'",
        deployment_name, pageserver_id
    );

    // In a real implementation, this would call the storage-controller API
    // to initiate tenant migration away from this pageserver

    Ok(())
}

fn create_pageserver_deployment_spec(
    namespace: &str,
    deployment_name: &str,
    neon_pageserver: &NeonPageserver,
    pageserver_id: &str,
    bucket_credentials_secret: &str,
    labels: BTreeMap<String, String>,
    oref: &OwnerReference,
) -> Deployment {
    Deployment {
        metadata: ObjectMeta {
            name: Some(deployment_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            finalizers: Some(vec![PAGESERVER_FINALIZER.to_string()]),
            owner_references: Some(vec![oref.clone()]),
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
                    init_containers: Some(vec![Container {
                        name: "setup-config".to_string(),
                        image: Some("busybox:latest".to_string()),
                        command: Some(vec!["/bin/sh".to_string(), "-c".to_string()]),
                        args: Some(vec![format!(
                            r#"
                    # Use the pageserver ID directly
                    echo "id={1}" > /config/identity.toml

                    # Create metadata.json with proper host information using service DNS
                    echo "{{\"host\":\"{0}-pageserver-{1}.{2}\",\"http_host\":\"{0}-pageserver-{1}.{2}\",\"http_port\":9898,\"port\":6400,\"availability_zone_id\":\"se-ume\"}}" > /config/metadata.json

                    # Copy pageserver.toml from configmap
                    cp /configmap/pageserver.toml /config/pageserver.toml
                    "#,
                            neon_pageserver.spec.cluster, pageserver_id, namespace
                        )]),
                        volume_mounts: Some(vec![
                            VolumeMount {
                                name: "pageserver-config".to_string(),
                                mount_path: "/configmap".to_string(),
                                ..Default::default()
                            },
                            VolumeMount {
                                name: "config".to_string(),
                                mount_path: "/config".to_string(),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }]),
                    containers: vec![Container {
                        name: "pageserver".to_string(),
                        image: Some("neondatabase/neon:7894".to_string()),
                        image_pull_policy: Some("Always".to_string()),
                        command: Some(vec!["/usr/local/bin/pageserver".to_string()]),
                        ports: Some(vec![
                            ContainerPort {
                                container_port: 6400,
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: 9898,
                                ..Default::default()
                            },
                        ]),
                        env: Some(vec![
                            EnvVar {
                                name: "RUST_LOG".to_string(),
                                value: Some("debug".to_string()),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "DEFAULT_PG_VERSION".to_string(),
                                value: Some("16".to_string()),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "AWS_ACCESS_KEY_ID".to_string(),
                                value_from: Some(EnvVarSource {
                                    secret_key_ref: Some(SecretKeySelector {
                                        key: "AWS_ACCESS_KEY_ID".to_string(),
                                        name: bucket_credentials_secret.to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "AWS_SECRET_ACCESS_KEY".to_string(),
                                value_from: Some(EnvVarSource {
                                    secret_key_ref: Some(SecretKeySelector {
                                        key: "AWS_SECRET_ACCESS_KEY".to_string(),
                                        name: bucket_credentials_secret.to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "AWS_REGION".to_string(),
                                value_from: Some(EnvVarSource {
                                    secret_key_ref: Some(SecretKeySelector {
                                        key: "AWS_REGION".to_string(),
                                        name: bucket_credentials_secret.to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "BUCKET_NAME".to_string(),
                                value_from: Some(EnvVarSource {
                                    secret_key_ref: Some(SecretKeySelector {
                                        key: "BUCKET_NAME".to_string(),
                                        name: bucket_credentials_secret.to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "AWS_ENDPOINT_URL".to_string(),
                                value_from: Some(EnvVarSource {
                                    secret_key_ref: Some(SecretKeySelector {
                                        key: "AWS_ENDPOINT_URL".to_string(),
                                        name: bucket_credentials_secret.to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                        ]),
                        volume_mounts: Some(vec![
                            VolumeMount {
                                name: "pageserver-storage".to_string(),
                                mount_path: "/data/.neon/tenants".to_string(),
                                ..Default::default()
                            },
                            VolumeMount {
                                name: "config".to_string(),
                                mount_path: "/data/.neon".to_string(),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }],
                    security_context: Some(PodSecurityContext {
                        run_as_user: Some(1000),
                        run_as_group: Some(1000),
                        fs_group: Some(1000),
                        ..Default::default()
                    }),
                    volumes: Some(vec![
                        Volume {
                            name: "pageserver-storage".to_string(),
                            persistent_volume_claim: Some(
                                k8s_openapi::api::core::v1::PersistentVolumeClaimVolumeSource {
                                    claim_name: deployment_name.to_string(),
                                    ..Default::default()
                                },
                            ),
                            ..Default::default()
                        },
                        Volume {
                            name: "pageserver-config".to_string(),
                            config_map: Some(ConfigMapVolumeSource {
                                name: deployment_name.to_string(),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        Volume {
                            name: "config".to_string(),
                            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource {
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
        ..Default::default()
    }
}
