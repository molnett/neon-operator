use crate::{
    api::v1::neoncluster::NeonCluster,
    controllers::cluster_controller::Context,
    util::errors::{self, Error, ErrorWithRequeue, Result, StdError},
};

use k8s_openapi::{
    api::{
        apps::v1::{Deployment, DeploymentSpec},
        core::v1::{Container, ContainerPort, PodSpec, PodTemplateSpec, Service, ServicePort, ServiceSpec},
    },
    apimachinery::pkg::apis::meta::v1::{LabelSelector, OwnerReference},
};
use kube::{
    api::{Api, ObjectMeta, Patch, PatchParams, PostParams},
    client::Client,
    runtime::controller::Action,
    Resource,
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::time::Duration;
use tracing::info;

pub async fn reconcile(neon_cluster: &NeonCluster, ctx: Arc<Context>) -> Result<Action> {
    let name = match &neon_cluster.metadata.name {
        Some(name) => format!("storage-broker-{}", name),
        None => {
            return Err(Error::ErrorWithRequeue(ErrorWithRequeue::new(
                StdError::MetadataMissing("Name should always be set on an existing object".to_string()),
                Duration::from_secs(5 * 60),
            )));
        }
    };

    let ns = neon_cluster.metadata.namespace.clone().unwrap();

    info!("Reconciling Storage Broker '{}' in namespace '{}'", name, ns);

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

    reconcile_deployment(&ctx.client, &ns, &name, &oref).await?;
    reconcile_service(&ctx.client, &ns, &name, &oref).await?;

    Ok(Action::requeue(Duration::from_secs(5 * 60)))
}

async fn reconcile_deployment(
    client: &Client,
    namespace: &str,
    name: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let desired_deployment = create_desired_deployment(namespace, name, oref);

    match deployments.get(name).await {
        Ok(existing) => {
            if deployment_needs_update(&existing, &desired_deployment) {
                info!("Updating Deployment '{}'", name);
                deployments
                    .patch(
                        name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_deployment),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("Deployment '{}' is up to date", name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating Deployment '{}'", name);
            deployments
                .create(&PostParams::default(), &desired_deployment)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(errors::Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

async fn reconcile_service(
    client: &Client,
    namespace: &str,
    service_name: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let desired_service = create_desired_service(namespace, service_name, oref);

    match services.get(service_name).await {
        Ok(existing) => {
            if service_needs_update(&existing, &desired_service) {
                info!("Updating Service '{}'", service_name);
                services
                    .patch(
                        service_name,
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
        Err(e) => return Err(errors::Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

fn create_desired_deployment(namespace: &str, name: &str, oref: &OwnerReference) -> Deployment {
    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), name.to_string());

    Deployment {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
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
                    containers: vec![Container {
                        name: "storage-broker".to_string(),
                        image: Some("neondatabase/neon:7894".to_string()),
                        command: Some(vec!["/usr/local/bin/storage_broker".to_string()]),
                        args: Some(vec!["--listen-addr=0.0.0.0:50051".to_string()]),
                        ports: Some(vec![ContainerPort {
                            container_port: 50051,
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
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
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                port: 50051,
                target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                    50051,
                )),
                protocol: Some("TCP".to_string()),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn deployment_needs_update(existing: &Deployment, desired: &Deployment) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.replicas != desired_spec.replicas
        || existing_spec.template.spec.as_ref().unwrap().containers[0].image
            != desired_spec.template.spec.as_ref().unwrap().containers[0].image
        || existing_spec.template.spec.as_ref().unwrap().containers[0].command
            != desired_spec.template.spec.as_ref().unwrap().containers[0].command
        || existing_spec.template.spec.as_ref().unwrap().containers[0].args
            != desired_spec.template.spec.as_ref().unwrap().containers[0].args
}

fn service_needs_update(existing: &Service, desired: &Service) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.selector != desired_spec.selector || existing_spec.ports != desired_spec.ports
}
