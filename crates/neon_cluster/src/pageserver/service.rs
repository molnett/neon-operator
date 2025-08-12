use crate::util::errors::{Error, Result, StdError};

use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

use kube::{
    api::{Patch, PatchParams, PostParams},
    Api, Client,
};

use std::collections::BTreeMap;
use tracing::info;

pub async fn reconcile_pageserver_service(
    client: &Client,
    name: &str,
    namespace: &str,
    pageserver_id: &str,
    oref: &OwnerReference,
) -> Result<()> {
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let desired_service = create_desired_pageserver_service(namespace, name, pageserver_id, oref);

    match services.get(name).await {
        Ok(existing) => {
            if service_needs_update(&existing, &desired_service) {
                info!("Updating Pageserver Service '{}'", name);
                services
                    .patch(
                        name,
                        &PatchParams::apply("kube-rs-controller").force(),
                        &Patch::Apply(&desired_service),
                    )
                    .await
                    .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
            } else {
                info!("Pageserver Service '{}' is up to date", name);
            }
        }
        Err(kube::Error::Api(api_err)) if api_err.code == 404 => {
            info!("Creating Pageserver Service '{}'", name);
            services
                .create(&PostParams::default(), &desired_service)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;
        }
        Err(e) => return Err(Error::StdError(StdError::KubeError(e))),
    }

    Ok(())
}

fn create_desired_pageserver_service(
    namespace: &str,
    name: &str,
    pageserver_id: &str,
    oref: &OwnerReference,
) -> Service {
    let mut labels = BTreeMap::new();
    labels.insert("app.kubernetes.io/name".to_string(), name.to_string());
    labels.insert(
        "app.kubernetes.io/component".to_string(),
        "pageserver".to_string(),
    );
    labels.insert("neon.io/pageserver-id".to_string(), pageserver_id.to_string());

    Service {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            owner_references: Some(vec![oref.clone()]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels),
            ports: Some(vec![
                ServicePort {
                    name: Some("pageserver-pg".to_string()),
                    port: 6400,
                    target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                        6400,
                    )),
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("pageserver-http".to_string()),
                    port: 9898,
                    target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                        9898,
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

pub fn service_needs_update(existing: &Service, desired: &Service) -> bool {
    let existing_spec = existing.spec.as_ref().unwrap();
    let desired_spec = desired.spec.as_ref().unwrap();

    existing_spec.cluster_ip != desired_spec.cluster_ip
        || existing_spec.selector != desired_spec.selector
        || existing_spec.ports != desired_spec.ports
}
