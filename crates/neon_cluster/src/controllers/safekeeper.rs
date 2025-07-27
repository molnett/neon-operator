use crate::util::errors::{Error, ErrorWithRequeue, Result, StdError};

use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, ObjectFieldSelector, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, PodSecurityContext, PodSpec, PodTemplateSpec, Service, ServicePort,
    ServiceSpec, VolumeMount, VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};

use kube::runtime::controller::Action;
use kube::{
    api::{Patch, PatchParams, PostParams},
    Api, Client, Resource, ResourceExt,
};

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::info;

use super::cluster_controller::Context;
use super::resources::*;

pub async fn reconcile(neon_cluster: &NeonCluster, ctx: Arc<Context>) -> Result<Action> {
    let name = match &neon_cluster.metadata.name {
        Some(name) => format!("safekeeper-{}", name),
        None => {
            return Err(Error::ErrorWithRequeue(ErrorWithRequeue::new(
                StdError::MetadataMissing("Name should always be set on an existing object".to_string()),
                Duration::from_secs(5 * 60),
            )));
        }
    };

    let ns = neon_cluster.namespace().unwrap_or_default();
    info!("Reconciling StatefulSet '{}' in namespace '{}'", name, ns);

    let oref = neon_cluster
        .controller_owner_ref(&())
        .unwrap_or_else(|| OwnerReference {
            api_version: "oltp.molnett.org/v1".to_string(),
            kind: "NeonCluster".to_string(),
            controller: Some(true),
            name: neon_cluster.metadata.name.clone().unwrap(),
            uid: format!("statefulset-{}", neon_cluster.metadata.name.clone().unwrap()),
            ..Default::default()
        });

    reconcile_statefulset(&ctx.client, &ns, &name, &neon_cluster, &oref).await?;
    reconcile_services(&ctx.client, &ns, &name, &oref).await?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

async fn reconcile_statefulset(
    client: &Client,
    namespace: &str,
    name: &str,
    neon_cluster: &NeonCluster,
    oref: &OwnerReference,
) -> Result<()> {
    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
    let desired_statefulset = create_desired_statefulset(namespace, name, neon_cluster, oref);

    match statefulsets.get(name).await {
        Ok(existing) => {
            if statefulset_needs_update(&existing, &desired_statefulset) {
                info!("Updating StatefulSet '{}'", name);
                statefulsets
                    .patch(
                        name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_statefulset),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("StatefulSet '{}' is up to date", name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating StatefulSet '{}'", name);
            statefulsets
                .create(&PostParams::default(), &desired_statefulset)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn reconcile_services(
    client: &Client,
    namespace: &str,
    name: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);

    let statefulset = statefulsets
        .get(name)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    let replicas = statefulset
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(1);

    for i in 0..replicas {
        let pod_name = format!("{}-{}", name, i);
        let service_name = format!("{}-{}", name, i);
        let desired_service = create_desired_service(namespace, &service_name, &pod_name, oref);

        match services.get(&service_name).await {
            Ok(existing) => {
                if service_needs_update(&existing, &desired_service) {
                    info!("Updating Service '{}'", service_name);
                    services
                        .patch(
                            &service_name,
                            &PatchParams::apply("kube-rs-controller").force(),
                            &Patch::Apply(&desired_service),
                        )
                        .await
                        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
                } else {
                    info!("Service '{}' is up to date", service_name);
                }
            }
            Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
                info!("Creating Service '{}'", service_name);
                services
                    .create(&PostParams::default(), &desired_service)
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            }
            Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
        }
    }

    Ok(())
}

fn create_desired_service(namespace: &str, name: &str, pod_name: &str, oref: &OwnerReference) -> Service {
    let mut selector = BTreeMap::new();
    selector.insert(
        "statefulset.kubernetes.io/pod-name".to_string(),
        pod_name.to_string(),
    );

    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![
                ServicePort {
                    name: Some("pg".to_string()),
                    port: 5454,
                    target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                        5454,
                    )),
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("http".to_string()),
                    port: 7676,
                    target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                        7676,
                    )),
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn create_desired_statefulset(
    namespace: &str,
    name: &str,
    neon_cluster: &NeonCluster,
    oref: &OwnerReference,
) -> StatefulSet {
    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), "safekeeper".to_string());

    let safekeeper_command = format!(
        "/usr/local/bin/safekeeper --id=$(echo ${{POD_NAME##*-}} | tr -d '-') --broker-endpoint=http://storage-broker-{}:50051 --listen-pg=0.0.0.0:5454 --listen-http=0.0.0.0:7676 --advertise-pg=${{POD_NAME}}:5454 --datadir /data",
        neon_cluster.metadata.name.clone()
            .expect("Kubernetes object without name in Metadata")
            .as_str(),
    );

    StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            service_name: Some("safekeeper".to_string()),
            replicas: Some(neon_cluster.spec.num_safekeepers as i32),
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
                    security_context: Some(PodSecurityContext {
                        run_as_user: Some(1000),
                        run_as_group: Some(1000),
                        fs_group: Some(1000),
                        ..Default::default()
                    }),
                    containers: vec![Container {
                        name: "safekeeper".to_string(),
                        image: Some("neondatabase/neon:7894".to_string()),
                        command: Some(vec!["/bin/bash".to_string()]),
                        args: Some(vec!["-c".to_string(), safekeeper_command]),
                        ports: Some(vec![
                            ContainerPort {
                                container_port: 5454,
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: 7676,
                                ..Default::default()
                            },
                        ]),
                        env: Some(vec![
                            EnvVar {
                                name: "DEFAULT_PG_VERSION".to_string(),
                                value: Some("15".to_string()),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "POD_NAME".to_string(),
                                value_from: Some(EnvVarSource {
                                    field_ref: Some(ObjectFieldSelector {
                                        field_path: "metadata.name".to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                        ]),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "safekeeper-storage".to_string(),
                            mount_path: "/data".to_string(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            volume_claim_templates: Some(vec![PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("safekeeper-storage".to_string()),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    storage_class_name: neon_cluster.spec.safekeeper_storage.storage_class.clone(),
                    resources: Some(VolumeResourceRequirements {
                        requests: Some({
                            let mut map = std::collections::BTreeMap::new();
                            map.insert(
                                "storage".to_string(),
                                k8s_openapi::apimachinery::pkg::api::resource::Quantity(
                                    neon_cluster.spec.safekeeper_storage.size.clone(),
                                ),
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

fn statefulset_needs_update(existing: &StatefulSet, desired: &StatefulSet) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.replicas != desired_spec.replicas
        || existing_spec.service_name != desired_spec.service_name
        || existing_spec.template.spec.as_ref().unwrap().containers[0].image
            != desired_spec.template.spec.as_ref().unwrap().containers[0].image
        || existing_spec.template.spec.as_ref().unwrap().containers[0].command
            != desired_spec.template.spec.as_ref().unwrap().containers[0].command
        || existing_spec.template.spec.as_ref().unwrap().containers[0].args
            != desired_spec.template.spec.as_ref().unwrap().containers[0].args
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
