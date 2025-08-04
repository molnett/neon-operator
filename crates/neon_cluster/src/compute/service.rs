use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Resource;

use crate::controllers::resources::{NeonBranch, NeonProject};

pub fn create_admin_service(name: &str, branch: &NeonBranch, project: &NeonProject) -> Service {
    let service_name = format!("{}-admin", name);

    let labels = BTreeMap::from([
        (
            "neon.timeline_id".to_string(),
            branch
                .spec
                .timeline_id
                .clone()
                .expect("a branch will always have a timeline ID"),
        ),
        (
            "neon.tenant_id".to_string(),
            project
                .spec
                .tenant_id
                .clone()
                .expect("a project will always have a tenant ID"),
        ),
    ]);

    Service {
        metadata: ObjectMeta {
            name: Some(service_name),
            owner_references: branch.controller_owner_ref(&()).map(|owner_ref| vec![owner_ref]),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(
                [("app".to_string(), format!("{}-compute-node", name))]
                    .into_iter()
                    .collect(),
            ),
            ports: Some(vec![ServicePort {
                name: Some("admin".to_string()),
                port: 3080,
                target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                    3080,
                )),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn create_postgres_service(name: &str, branch: &NeonBranch, project: &NeonProject) -> Service {
    let service_name = format!("{}-postgres", name);
    let labels = BTreeMap::from([
        (
            "neon.timeline_id".to_string(),
            branch
                .spec
                .timeline_id
                .clone()
                .expect("a branch will always have a timeline ID"),
        ),
        (
            "neon.tenant_id".to_string(),
            project
                .spec
                .tenant_id
                .clone()
                .expect("a project will always have a tenant ID"),
        ),
    ]);

    Service {
        metadata: ObjectMeta {
            name: Some(service_name),
            owner_references: branch.controller_owner_ref(&()).map(|owner_ref| vec![owner_ref]),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(
                [("app".to_string(), format!("{}-compute-node", name))]
                    .into_iter()
                    .collect(),
            ),
            ports: Some(vec![ServicePort {
                name: Some("postgres".to_string()),
                port: 55433,
                target_port: Some(k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                    55433,
                )),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}
