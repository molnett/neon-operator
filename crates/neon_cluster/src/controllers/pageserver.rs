use crate::util::errors::{Error, ErrorWithRequeue, Result, StdError};

use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{
    ConfigMap, ConfigMapVolumeSource, Container, ContainerPort, EnvVar, EnvVarSource, PodSpec,
    PodTemplateSpec, SecretKeySelector, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};

use kube::runtime::controller::Action;
use kube::{
    api::{ListParams, Patch, PatchParams, PostParams},
    Api, Client, Resource, ResourceExt,
};

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{error, info, instrument, warn};

use super::cluster_controller::Context;
use super::resources::*;
pub async fn reconcile(neon_cluster: &NeonCluster, ctx: Arc<Context>) -> Result<Action> {
    let name = match &neon_cluster.metadata.name {
        Some(name) => format!("pageserver-{}", name),
        None => {
            return Err(Error::ErrorWithRequeue(ErrorWithRequeue::new(
                StdError::IllegalDocument,
                Duration::from_secs(5 * 60),
            )));
        }
    };

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

    reconcile_configmap(&ctx.client, &ns, &name, &neon_cluster.name_any(), &oref).await?;
    reconcile_statefulset(
        &ctx.client,
        &ns,
        &name,
        &neon_cluster.spec.bucket_credentials_secret,
        &oref,
    )
    .await?;
    reconcile_service(&ctx.client, &ns, &name, &oref).await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn reconcile_configmap(
    client: &Client,
    namespace: &str,
    name: &str,
    cluster_name: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let desired_configmap = create_desired_configmap(namespace, name, cluster_name, oref);

    match configmaps.get(name).await {
        Ok(existing) => {
            if configmap_needs_update(&existing, &desired_configmap) {
                info!("Updating ConfigMap '{}'", name);
                configmaps
                    .patch(
                        name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_configmap),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("ConfigMap '{}' is up to date", name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating ConfigMap '{}'", name);
            configmaps
                .create(&PostParams::default(), &desired_configmap)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn reconcile_statefulset(
    client: &Client,
    namespace: &str,
    cluster_name: &str,
    bucket_credentials_secret: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
    let desired_statefulset =
        create_desired_statefulset(namespace, cluster_name, bucket_credentials_secret, oref);

    match statefulsets.get(cluster_name).await {
        Ok(existing) => {
            if statefulset_needs_update(&existing, &desired_statefulset) {
                info!("Updating StatefulSet '{}'", cluster_name);
                statefulsets
                    .patch(
                        cluster_name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_statefulset),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("StatefulSet '{}' is up to date", cluster_name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating StatefulSet '{}'", cluster_name);
            statefulsets
                .create(&PostParams::default(), &desired_statefulset)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn reconcile_service(
    client: &Client,
    namespace: &str,
    name: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let desired_service = create_desired_service(namespace, name, oref);

    match services.get(name).await {
        Ok(existing) => {
            if service_needs_update(&existing, &desired_service) {
                info!("Updating Service '{}'", name);
                services
                    .patch(
                        name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_service),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("Service '{}' is up to date", name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating Service '{}'", name);
            services
                .create(&PostParams::default(), &desired_service)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

fn create_desired_configmap(
    namespace: &str,
    name: &str,
    cluster_name: &str,
    oref: &OwnerReference,
) -> ConfigMap {
    ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        data: Some({
            let mut data = BTreeMap::new();
            data.insert(
                "pageserver.toml".to_string(),
                format!(
                    r#"
                        listen_pg_addr = "0.0.0.0:6400"
                        broker_endpoint = "http://storage-broker-{}:50051"
                        pg_distrib_dir='/usr/local/'
                        [remote_storage]
                        bucket_name = "neon-operator"
                        bucket_region = ""
                        endpoint = "https://fly.storage.tigris.dev"
                    "#,
                    cluster_name
                )
                .to_string(),
            );
            data.insert("identity.toml".to_string(), "id=1234".to_string());
            data
        }),
        ..Default::default()
    }
}

fn create_desired_statefulset(
    namespace: &str,
    cluster_name: &str,
    bucket_credentials_secret: &str,
    oref: &OwnerReference,
) -> StatefulSet {
    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), cluster_name.to_string());
    StatefulSet {
        metadata: ObjectMeta {
            name: Some(cluster_name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            service_name: "pageserver".to_string(),
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
                        name: "pageserver".to_string(),
                        image: Some("neondatabase/neon:6351-bookworm".to_string()),
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
                                        name: Some(bucket_credentials_secret.to_string()),
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
                                        name: Some(bucket_credentials_secret.to_string()),
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
                                name: "pageserver-config".to_string(),
                                mount_path: "/data/.neon/pageserver.toml".to_string(),
                                sub_path: Some("pageserver.toml".to_string()),
                                ..Default::default()
                            },
                            VolumeMount {
                                name: "pageserver-config".to_string(),
                                mount_path: "/data/.neon/identity.toml".to_string(),
                                sub_path: Some("identity.toml".to_string()),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![
                        Volume {
                            name: "pageserver-storage".to_string(),
                            persistent_volume_claim: Some(
                                k8s_openapi::api::core::v1::PersistentVolumeClaimVolumeSource {
                                    claim_name: "pageserver-storage".to_string(),
                                    ..Default::default()
                                },
                            ),
                            ..Default::default()
                        },
                        Volume {
                            name: "pageserver-config".to_string(),
                            config_map: Some(ConfigMapVolumeSource {
                                name: Some(cluster_name.to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
            },
            volume_claim_templates: Some(vec![k8s_openapi::api::core::v1::PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("pageserver-storage".to_string()),
                    ..Default::default()
                },
                spec: Some(k8s_openapi::api::core::v1::PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    resources: Some(k8s_openapi::api::core::v1::ResourceRequirements {
                        requests: Some({
                            let mut map = BTreeMap::new();
                            map.insert(
                                "storage".to_string(),
                                k8s_openapi::apimachinery::pkg::api::resource::Quantity("10Gi".to_string()),
                            );
                            map
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn create_desired_service(namespace: &str, name: &str, oref: &OwnerReference) -> Service {
    let mut selector = BTreeMap::new();
    selector.insert("app.kubernetes.io/name".to_string(), name.to_string());

    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            cluster_ip: Some("None".to_string()),
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                port: 6400,
                target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                    6400,
                )),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn configmap_needs_update(existing: &ConfigMap, desired: &ConfigMap) -> bool {
    existing.data != desired.data
}

fn statefulset_needs_update(existing: &StatefulSet, desired: &StatefulSet) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.replicas != desired_spec.replicas
        || existing_spec.service_name != desired_spec.service_name
        || existing_spec.template.spec.as_ref().unwrap().containers[0].image
            != desired_spec.template.spec.as_ref().unwrap().containers[0].image
        || existing_spec.template.spec.as_ref().unwrap().containers[0].command
            != desired_spec.template.spec.as_ref().unwrap().containers[0].command
        || existing_spec.template.spec.as_ref().unwrap().containers[0].env
            != desired_spec.template.spec.as_ref().unwrap().containers[0].env
        || existing_spec.template.spec.as_ref().unwrap().containers[0].volume_mounts
            != desired_spec.template.spec.as_ref().unwrap().containers[0].volume_mounts
        || existing_spec.volume_claim_templates != desired_spec.volume_claim_templates
}

fn service_needs_update(existing: &Service, desired: &Service) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.cluster_ip != desired_spec.cluster_ip
        || existing_spec.selector != desired_spec.selector
        || existing_spec.ports != desired_spec.ports
}
