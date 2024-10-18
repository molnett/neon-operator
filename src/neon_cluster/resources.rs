use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::{Deployment, DeploymentSpec, StatefulSet, StatefulSetSpec},
        core::v1::{
            ConfigMap, ConfigMapVolumeSource, Container, ContainerPort, EnvVar, EnvVarSource,
            ObjectFieldSelector, PersistentVolumeClaim, PersistentVolumeClaimSpec,
            PersistentVolumeClaimTemplate, PodSpec, PodTemplateSpec, ResourceRequirements, Service,
            ServicePort, ServiceSpec, Volume, VolumeMount,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity,
        apis::meta::v1::{LabelSelector, OwnerReference},
        util::intstr::IntOrString,
    },
};
use kube::{api::ObjectMeta, runtime::reflector::ObjectRef};

use super::controller::NeonCluster;

pub fn safekeeper_statefulset(neon_cluster: &NeonCluster, oref: OwnerReference) -> StatefulSet {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "safekeeper".to_string());
    labels.insert(
        "neon-cluster".to_string(),
        neon_cluster
            .metadata
            .name
            .clone()
            .expect("NeonCluster requires a name"),
    );

    StatefulSet {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-safekeeper",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            replicas: Some(3),
            service_name: "safekeeper".to_string(),
            selector: LabelSelector {
                match_expressions: None,
                match_labels: Some(labels.clone()),
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "safekeeper".to_string(),
                        image: Some(neon_cluster.spec.neon_image.clone()),
                        command: Some(vec!["/bin/bash".to_string()]),
                        args: Some(vec!["-c".to_string(), format!("/usr/local/bin/safekeeper --id=$(echo ${{POD_NAME##*-}} | tr -d '-') --broker-endpoint=http://{}-storage-broker.{}.svc.cluster.local:50051 --listen-pg=0.0.0.0:5454 --listen-http=0.0.0.0:7676 --advertise-pg=${{POD_NAME}}.safekeeper:5454", neon_cluster.metadata.name.clone().expect("NeonCluster requires a name"), neon_cluster.metadata.namespace.clone().unwrap_or("default".to_string())).to_string()]),
                        ports: Some(vec![
                            ContainerPort{container_port: 5454, ..Default::default()},
                            ContainerPort{container_port: 7676, ..Default::default()}
                        ]),
                        env: Some(vec![
                            EnvVar {
                                name: "DEFAULT_PG_VERSION".to_string(),
                                value: Some("15".to_string()),
                                ..Default::default()
                            }, EnvVar {
                                name: "POD_NAME".to_string(),
                                value_from: Some(EnvVarSource {
                                    field_ref: Some(ObjectFieldSelector {
                                        field_path: "metadata.name".to_string(),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }
                        ]),
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

pub fn safekeeper_service(neon_cluster: &NeonCluster, oref: OwnerReference) -> Service {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "safekeeper".to_string());
    labels.insert(
        "neon-cluster".to_string(),
        neon_cluster
            .metadata
            .name
            .clone()
            .expect("NeonCluster requires a name"),
    );
    Service {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-safekeeper",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            cluster_ip: Some("None".to_string()),
            selector: Some(labels),
            ports: Some(vec![
                ServicePort {
                    name: Some("pg".to_string()),
                    port: 5454,
                    target_port: Some(IntOrString::Int(5454)),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("http".to_string()),
                    port: 7676,
                    target_port: Some(IntOrString::Int(7676)),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn storage_broker_deployment(neon_cluster: &NeonCluster, oref: OwnerReference) -> Deployment {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "storage-broker".to_string());
    labels.insert(
        "neon-cluster".to_string(),
        neon_cluster
            .metadata
            .name
            .clone()
            .expect("NeonCluster requires a name"),
    );

    Deployment {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-storage-broker",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },

        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_expressions: None,
                match_labels: Some(labels.clone()),
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "storage-broker".to_string(),
                        image: Some(neon_cluster.spec.neon_image.clone()),
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

pub fn storage_broker_service(neon_cluster: &NeonCluster, oref: OwnerReference) -> Service {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "storage-broker".to_string());
    labels.insert(
        "neon-cluster".to_string(),
        neon_cluster
            .metadata
            .name
            .clone()
            .expect("NeonCluster requires a name"),
    );

    Service {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-storage-broker",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels),
            ports: Some(vec![ServicePort {
                name: Some("storage-broker".to_string()),
                port: 50051,
                target_port: Some(IntOrString::Int(50051)),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn pageserver_statefulset(neon_cluster: &NeonCluster, oref: OwnerReference) -> StatefulSet {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "pageserver".to_string());
    labels.insert(
        "neon-cluster".to_string(),
        neon_cluster
            .metadata
            .name
            .clone()
            .expect("NeonCluster requires a name"),
    );

    StatefulSet {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-pageserver",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },
        spec: Some(StatefulSetSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_expressions: None,
                match_labels: Some(labels.clone()),
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "pageserver".to_string(),
                        image: Some(neon_cluster.spec.neon_image.clone()),
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
                        env: Some(vec![
                            EnvVar {
                                name: "RUST_LOG".to_string(),
                                value: Some("debug".to_string()),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "DEFAULT_PG_VERSION".to_string(),
                                value: Some(format!("{:?}", neon_cluster.spec.default_pg_version)),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "AWS_ACCESS_KEY_ID".to_string(),
                                value: Some("neon".to_string()),
                                ..Default::default()
                            },
                            EnvVar {
                                name: "AWS_SECRET_ACCESS_KEY".to_string(),
                                value: Some("neonneon".to_string()),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![Volume {
                        name: "pageserver-config".to_string(),
                        config_map: Some(ConfigMapVolumeSource {
                            name: Some(format!(
                                "{}-pageserver-config",
                                neon_cluster
                                    .metadata
                                    .name
                                    .clone()
                                    .expect("NeonCluster requires a name")
                            )),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            },
            volume_claim_templates: Some(vec![PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("pageserver-storage".to_string()),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                    resources: Some(ResourceRequirements {
                        requests: Some(BTreeMap::from([(
                            "storage".to_string(),
                            Quantity("10Gi".to_string()),
                        )])),
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

pub fn pageserver_service(neon_cluster: &NeonCluster, oref: OwnerReference) -> Service {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "pageserver".to_string());
    labels.insert(
        "neon-cluster".to_string(),
        neon_cluster
            .metadata
            .name
            .clone()
            .expect("NeonCluster requires a name"),
    );

    Service {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-pageserver",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels),
            ports: Some(vec![ServicePort {
                name: Some("pageserver".to_string()),
                port: 6420,
                target_port: Some(IntOrString::Int(6400)),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

// Pageserver CM
/*
apiVersion: v1
kind: ConfigMap
metadata:
  name: pageserver-config
  namespace: neon
data:
  identity.toml: |
    id=1234
  pageserver.toml: |
    broker_endpoint = 'http://storage-broker-service.neon.svc.cluster.local:50051'
    pg_distrib_dir = '/usr/local/'
    listen_pg_addr = '0.0.0.0:6400'
    listen_http_addr = '0.0.0.0:9898'

    [remote_storage]
    bucket_name = "neon"
    bucket_region = "eu-north-1"
    endpoint = "http://minio.neon.svc.cluster.local"
*/

pub fn pageserver_configmap(neon_cluster: &NeonCluster, oref: OwnerReference) -> ConfigMap {
    ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-pageserver-config",
                neon_cluster
                    .metadata
                    .name
                    .clone()
                    .expect("NeonCluster requires a name")
            )),
            namespace: neon_cluster.metadata.namespace.clone(),
            owner_references: Some(vec![oref]),
            ..Default::default()
        },
        data: Some(BTreeMap::from([
            ("identity.toml".to_string(), "id=1234".to_string()),
            (
                "pageserver.toml".to_string(),
                format!(
                    "broker_endpoint = 'http://storage-broker-service.{}.svc.cluster.local:50051'
                pg_distrib_dir = '/usr/local/'
                listen_pg_addr = '0.0.0.0:6400'
                listen_http_addr = '0.0.0.0:9898'

                [remote_storage]
                bucket_name = \"neon\"
                bucket_region = \"eu-north-1\"
                endpoint = \"http://minio.neon.svc.cluster.local\"",
                    neon_cluster
                        .metadata
                        .namespace
                        .clone()
                        .unwrap_or("default".to_string())
                )
                .to_string(),
            ),
        ])),
        ..Default::default()
    }
}
