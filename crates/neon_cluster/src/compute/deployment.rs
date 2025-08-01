use crate::controllers::resources::{NeonBranch, NeonProject};
use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, PodSecurityContext, PodSpec, PodTemplateSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use std::collections::BTreeMap;

pub fn create_compute_deployment(name: &str, branch: &NeonBranch, project: &NeonProject) -> Deployment {
    let deployment_name = format!("{}-compute-node", name);

    // Labels for pod selection and identification
    let labels = BTreeMap::from([
        ("app".to_string(), deployment_name.clone()),
        (
            "neon.tenant_id".to_string(),
            project.spec.tenant_id.clone().unwrap_or_default(),
        ),
        (
            "neon.timeline_id".to_string(),
            branch.spec.timeline_id.clone().unwrap_or_default(),
        ),
    ]);

    // Annotations to store metadata for spec generation
    let mut annotations = BTreeMap::new();
    annotations.insert("neon.compute_id".to_string(), name.to_string());
    annotations.insert("neon.cluster_name".to_string(), project.spec.cluster_name.clone());

    Deployment {
        metadata: ObjectMeta {
            name: Some(deployment_name.clone()),
            labels: Some(labels.clone()),
            annotations: Some(annotations),
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
                    labels: Some(labels.clone()),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    security_context: Some(PodSecurityContext {
                        run_as_user: Some(1000),
                        fs_group: Some(1000),
                        ..Default::default()
                    }),
                    containers: vec![Container {
                        name: "storage-broker".to_string(),
                        image: Some(format!("neondatabase/compute-node-v{}", branch.spec.pg_version)),
                        command: Some(vec![
                            "bash".to_string(),
                            "-c".to_string(),
                            format!(
                                "echo \"$INITIAL_SPEC_JSON\" > /var/spec.json && /usr/local/bin/compute_ctl --pgdata /.neon/data/pgdata --connstr=postgresql://cloud_admin:@0.0.0.0:55433/postgres --compute-id {} -p http://neon-operator.neon:8080 --pgbin /usr/local/bin/postgres",
                                name
                            ),
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
                        env: Some(vec![
                            EnvVar {
                                name: "OTEL_SDK_DISABLED".to_string(),
                                value: Some("true".to_string()),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "INITIAL_SPEC_JSON".to_string(),
                                value_from: Some(k8s_openapi::api::core::v1::EnvVarSource {
                                    config_map_key_ref: Some(k8s_openapi::api::core::v1::ConfigMapKeySelector {
                                        name: format!("{}-compute-spec", name),
                                        key: "spec.json".to_string(),
                                        optional: Some(false),
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![
                        Volume {
                            name: "spec-volume".to_string(),
                            empty_dir: Some(k8s_openapi::api::core::v1::EmptyDirVolumeSource {
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
