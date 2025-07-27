use crate::controllers::resources::NeonCluster;
use crate::util::errors::{Error, ErrorWithRequeue, Result, StdError};

use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, ContainerPort, EnvVar, EnvVarSource, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, Pod, PodSecurityContext, PodSpec, SecretKeySelector, SecurityContext, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

use kube::ResourceExt;
use kube::{
    api::{Patch, PatchParams, PostParams},
    Api, Client,
};

use rand::Rng;
use serde_json::json;
use std::collections::BTreeMap;
use tokio::time::Duration;
use tracing::{info, warn};

const PAGESERVER_FINALIZER: &str = "neon.io/drain-required";

// Generate a new unique pageserver ID by checking against existing IDs
pub fn generate_unique_pageserver_id(existing_ids: &[String]) -> String {
    let mut rng = rand::thread_rng();

    // Generate IDs in u32 range to avoid parsing issues in pageserver code
    // Still gives us 4+ billion unique values which is more than sufficient
    loop {
        let id: u32 = rng.gen();
        // Ensure we never generate 0 (in case pageserver treats 0 as special)
        if id == 0 {
            continue;
        }
        let id_str = id.to_string();

        // Check if this ID is already in use in this cluster
        if !existing_ids.contains(&id_str) {
            info!("Generated unique pageserver ID: {}", id_str);
            return id_str;
        }

        // If we hit a collision (extremely unlikely), try again
        warn!(
            "Generated pageserver ID {} already exists, generating new one",
            id_str
        );
    }
}

pub async fn reconcile_single_pageserver_pod(
    client: &Client,
    namespace: &str,
    neon_cluster: &NeonCluster,
    pageserver_id: &str,
    bucket_credentials_secret: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let pod_name = format!("pageserver-{}", pageserver_id);
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

    // Check if pod already exists
    match pods.get(&pod_name).await {
        Ok(_) => {
            info!("Pod '{}' already exists", pod_name);
            return Ok(());
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            // Pod doesn't exist, create PVC and then pod
            create_pageserver_pvc(client, namespace, neon_cluster, pageserver_id, oref).await?;
            create_pageserver_pod(
                client,
                namespace,
                neon_cluster,
                pageserver_id,
                bucket_credentials_secret,
                oref,
            )
            .await?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn create_pageserver_pvc(
    client: &Client,
    namespace: &str,
    neon_cluster: &NeonCluster,
    pageserver_id: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let pvc_name = format!("pageserver-{}-storage", pageserver_id);
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);

    // Check if PVC already exists
    match pvcs.get(&pvc_name).await {
        Ok(_) => {
            info!("PVC '{}' already exists", pvc_name);
            return Ok(());
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            // Create PVC
            let mut labels = BTreeMap::new();
            labels.insert(
                "app.kubernetes.io/name".to_string(),
                format!(
                    "pageserver-{}",
                    neon_cluster
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
                    name: Some(pvc_name.clone()),
                    namespace: Some(namespace.to_string()),
                    labels: Some(labels),
                    owner_references: Some(vec![oref.clone()]),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    storage_class_name: neon_cluster.spec.pageserver_storage.storage_class.clone(),
                    resources: Some(k8s_openapi::api::core::v1::VolumeResourceRequirements {
                        requests: Some({
                            let mut map = BTreeMap::new();
                            map.insert(
                                "storage".to_string(),
                                Quantity(neon_cluster.spec.pageserver_storage.size.clone()),
                            );
                            map
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            };

            info!("Creating PVC '{}'", pvc_name);
            pvcs.create(&PostParams::default(), &pvc)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn create_pageserver_pod(
    client: &Client,
    namespace: &str,
    neon_cluster: &NeonCluster,
    pageserver_id: &str,
    bucket_credentials_secret: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let cluster_name = neon_cluster
        .metadata
        .name
        .clone()
        .expect("Kubernetes object without name in Metadata");
    let pod_name = format!("pageserver-{}", pageserver_id);
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
    labels.insert("neon.io/pageserver-id".to_string(), pageserver_id.to_string());

    let pod = create_pageserver_pod_spec(
        namespace,
        &pod_name,
        neon_cluster,
        pageserver_id,
        bucket_credentials_secret,
        labels,
        oref,
    );

    info!("Creating Pod '{}'", pod_name);
    pods.create(&PostParams::default(), &pod)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    Ok(())
}

pub async fn handle_pageserver_deletion(pod: &Pod, client: &Client) -> Result<()> {
    let pod_name = pod.metadata.name.as_ref().unwrap();
    let namespace = pod.metadata.namespace.as_ref().unwrap();

    // Check if pod has our finalizer
    if let Some(finalizers) = &pod.metadata.finalizers {
        if !finalizers.contains(&PAGESERVER_FINALIZER.to_string()) {
            return Ok(());
        }
    } else {
        return Ok(());
    }

    info!("Handling deletion for pageserver pod '{}'", pod_name);

    // Check if pageserver is drained
    if !is_pageserver_drained(pod).await? {
        warn!("Pageserver '{}' is not drained, triggering drain", pod_name);
        trigger_pageserver_drain(pod).await?;
        // Return error to requeue
        return Err(Error::ErrorWithRequeue(ErrorWithRequeue::new(
            StdError::InvalidArgument("Waiting for pageserver drain".to_string()),
            Duration::from_secs(30),
        )));
    }

    // Remove finalizer
    info!("Pageserver '{}' is drained, removing finalizer", pod_name);
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let finalizers: Vec<String> = pod
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

    pods.patch(pod_name, &PatchParams::default(), &Patch::Merge(patch))
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    Ok(())
}

async fn is_pageserver_drained(pod: &Pod) -> Result<bool> {
    // TODO: Implement actual drain check via storage-controller API (#21)
    // For now, we'll check if a specific annotation is set
    if let Some(annotations) = &pod.metadata.annotations {
        return Ok(annotations.get("neon.io/drained") == Some(&"true".to_string()));
    }
    Ok(false)
}

async fn trigger_pageserver_drain(pod: &Pod) -> Result<()> {
    // TODO: Implement actual drain trigger via storage-controller API (#21)
    // For now, we'll just log
    let pod_name = pod.metadata.name.as_ref().unwrap();
    let pageserver_id = pod
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get("neon.io/pageserver-id"))
        .map(|s| s.as_str())
        .unwrap_or("unknown");

    info!(
        "Triggering drain for pageserver '{}' with ID '{}'",
        pod_name, pageserver_id
    );

    // In a real implementation, this would call the storage-controller API
    // to initiate tenant migration away from this pageserver

    Ok(())
}

fn create_pageserver_pod_spec(
    namespace: &str,
    pod_name: &str,
    neon_cluster: &NeonCluster,
    pageserver_id: &str,
    bucket_credentials_secret: &str,
    labels: BTreeMap<String, String>,
    oref: &OwnerReference,
) -> Pod {
    let cluster_name = neon_cluster
        .metadata
        .name
        .clone()
        .expect("Kubernetes object without name in Metadata");
    Pod {
        metadata: ObjectMeta {
            name: Some(pod_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            finalizers: Some(vec![PAGESERVER_FINALIZER.to_string()]),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        spec: Some(PodSpec {
            init_containers: Some(vec![Container {
                name: "setup-config".to_string(),
                image: Some("busybox:latest".to_string()),
                command: Some(vec!["/bin/sh".to_string(), "-c".to_string()]),
                args: Some(vec![format!(
                    r#"
                    # Use the pageserver ID directly
                    echo "id={0}" > /config/identity.toml

                    # Create metadata.json with proper host information using specific pod through headless service
                    echo "{{\"host\":\"{1}.pageserver-{2}-headless.{3}\",\"http_host\":\"{1}.pageserver-{2}-headless.{3}\",\"http_port\":9898,\"port\":6400,\"availability_zone_id\":\"se-ume\"}}" > /config/metadata.json

                    # Copy pageserver.toml from configmap
                    cp /configmap/pageserver.toml /config/pageserver.toml
                    "#,
                    pageserver_id, pod_name, cluster_name, namespace
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
                            claim_name: format!("pageserver-{}-storage", pageserver_id),
                            ..Default::default()
                        },
                    ),
                    ..Default::default()
                },
                Volume {
                    name: "pageserver-config".to_string(),
                    config_map: Some(ConfigMapVolumeSource {
                        name: format!("pageserver-{}", cluster_name),
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
            hostname: Some(pod_name.to_string()),
            subdomain: Some(format!("pageserver-{}-headless", cluster_name)),
            ..Default::default()
        }),
        ..Default::default()
    }
}
