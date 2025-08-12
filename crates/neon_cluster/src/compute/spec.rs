use std::collections::HashMap;
use std::str::FromStr;

use crate::api::v1::neonbranch::NeonBranch;
use crate::api::v1::neonproject::NeonProject;
use crate::api::v1::NodeId;
use crate::storage_controller::client::StorageControllerClient;
use crate::util::errors::{Error, Result, StdError};
use crate::util::secrets::get_jwt_keys_from_secret;
use k8s_openapi::api::apps::v1::Deployment;
use kube::api::{Api, ListParams};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info};

/// Shard information for compute hook notification
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct ComputeHookNotifyRequestShard {
    pub node_id: NodeId,
    pub shard_number: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ComputeHookNotifyRequest {
    pub tenant_id: String,
    pub stripe_size: Option<u32>,
    pub shards: Vec<ComputeHookNotifyRequestShard>,
}

/*
 * The following types are vendored from the upstream neon repository
 * https://github.com/neondatabase/neon/blob/main/libs/utils/src/shard.rs
 *
 * START
 */

#[derive(Eq, PartialEq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct ShardIndex {
    pub shard_number: u8,
    pub shard_count: u8,
}

impl std::fmt::Display for ShardIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02x}{:02x}", self.shard_number, self.shard_count)
    }
}

impl std::fmt::Debug for ShardIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Debug is the same as Display: the compact hex representation
        write!(f, "{self}")
    }
}

impl std::str::FromStr for ShardIndex {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Expect format: 1 byte shard number, 1 byte shard count
        if s.len() == 4 {
            let bytes = s.as_bytes();
            let mut shard_parts: [u8; 2] = [0u8; 2];
            hex::decode_to_slice(bytes, &mut shard_parts)?;
            Ok(Self {
                shard_number: shard_parts[0],
                shard_count: shard_parts[1],
            })
        } else {
            Err(hex::FromHexError::InvalidStringLength)
        }
    }
}

impl From<[u8; 2]> for ShardIndex {
    fn from(b: [u8; 2]) -> Self {
        Self {
            shard_number: b[0] as u8,
            shard_count: b[1] as u8,
        }
    }
}

impl Serialize for ShardIndex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            serializer.collect_str(self)
        } else {
            // Binary encoding is not used in index_part.json, but is included in anticipation of
            // switching various structures (e.g. inter-process communication, remote metadata) to more
            // compact binary encodings in future.
            let mut packed: [u8; 2] = [0; 2];
            packed[0] = self.shard_number;
            packed[1] = self.shard_count;
            packed.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for ShardIndex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct IdVisitor {
            is_human_readable_deserializer: bool,
        }

        impl<'de> serde::de::Visitor<'de> for IdVisitor {
            type Value = ShardIndex;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                if self.is_human_readable_deserializer {
                    formatter.write_str("value in form of hex string")
                } else {
                    formatter.write_str("value in form of integer array([u8; 2])")
                }
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let s = serde::de::value::SeqAccessDeserializer::new(seq);
                let id: [u8; 2] = Deserialize::deserialize(s)?;
                Ok(ShardIndex::from(id))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ShardIndex::from_str(v).map_err(E::custom)
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(IdVisitor {
                is_human_readable_deserializer: true,
            })
        } else {
            deserializer.deserialize_tuple(
                2,
                IdVisitor {
                    is_human_readable_deserializer: false,
                },
            )
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PageserverShardInfo {
    pub pageservers: Vec<PageserverShardConnectionInfo>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PageserverShardConnectionInfo {
    pub id: Option<NodeId>,
    pub libpq_url: Option<String>,
    pub grpc_url: Option<String>,
}

/*
 * END
 */

pub async fn generate_compute_spec(
    client: &kube::Client,
    request: Option<&ComputeHookNotifyRequest>,
    compute_id: &str,
) -> Result<serde_json::Value> {
    info!("Starting compute spec generation for compute_id: {}", compute_id);

    // 1. Find the compute deployment to get cluster context
    let deployment = match find_compute_deployment(client, compute_id).await {
        Ok(d) => d,
        Err(e) => {
            error!(
                "Failed to find compute deployment for compute_id {}: {}",
                compute_id, e
            );
            return Err(e);
        }
    };

    let cluster_name = match extract_cluster_name(&deployment) {
        Ok(name) => name,
        Err(e) => {
            error!("Failed to extract cluster name from deployment: {}", e);
            return Err(e);
        }
    };

    info!("Found cluster name: {}", cluster_name);

    let tenant_id = match deployment.labels().get("neon.tenant_id") {
        Some(id) => id,
        None => {
            error!("Deployment missing neon.tenant_id label");
            return Err(Error::StdError(StdError::MetadataMissing(
                "neon.tenant_id label not found".into(),
            )));
        }
    };

    let timeline_id = match deployment.labels().get("neon.timeline_id") {
        Some(id) => id,
        None => {
            error!("Deployment missing neon.timeline_id label");
            return Err(Error::StdError(StdError::MetadataMissing(
                "neon.timeline_id label not found".into(),
            )));
        }
    };

    info!("Found tenant_id: {}, timeline_id: {}", tenant_id, timeline_id);

    // 2. Get project and branch details
    let (project, branch) = match find_project_and_branch(client, &tenant_id, &timeline_id).await {
        Ok(result) => result,
        Err(e) => {
            error!(
                "Failed to find project and branch for tenant_id: {}, timeline_id: {}: {}",
                tenant_id, timeline_id, e
            );
            return Err(e);
        }
    };

    // 3. Get JWT keys from cluster secret
    let jwks = match get_jwt_keys_from_secret(client, &cluster_name).await {
        Ok(keys) => keys,
        Err(e) => {
            error!(
                "Failed to get JWT keys from secret for cluster {}: {}",
                cluster_name, e
            );
            return Err(e);
        }
    };

    info!("Successfully retrieved JWT keys");

    // 4. Construct safekeeper connections (always 3)
    let safekeeper_connstrings: Vec<String> = (0..3)
        .map(|i| {
            format!(
                "postgresql://postgres:@safekeeper-{}-{}.neon:5454",
                cluster_name, i
            )
        })
        .collect();

    // 5. Build postgres settings
    let settings = build_postgres_settings(
        &cluster_name,
        &project.spec.tenant_id.unwrap(),
        &branch.spec.timeline_id.unwrap(),
    );

    // 6. Generate spec

    let mut shards = HashMap::<ShardIndex, PageserverShardInfo>::new();

    let fallback_request;
    let actual_request = if let Some(req) = request {
        req
    } else {
        let client = StorageControllerClient::new(&cluster_name);
        let tenant_info = client.get_tenant_info(tenant_id).await?;

        fallback_request = ComputeHookNotifyRequest {
            tenant_id: tenant_info.tenant_id,
            stripe_size: Some(tenant_info.stripe_size),
            shards: tenant_info
                .shards
                .iter()
                .enumerate()
                .map(|(i, shard)| ComputeHookNotifyRequestShard {
                    node_id: shard.clone().node_attached,
                    shard_number: i as u32,
                })
                .collect(),
        };
        &fallback_request
    };

    for shard in actual_request.shards.clone().into_iter() {
        shards.insert(
            ShardIndex {
                shard_number: 0,
                shard_count: 0,
            },
            PageserverShardInfo {
                pageservers: vec![PageserverShardConnectionInfo {
                    id: Some(shard.node_id.clone()),
                    libpq_url: Some(format!(
                        "postgres://no_user@{}-pageserver-{}.neon:6400",
                        cluster_name, shard.node_id
                    )),
                    grpc_url: None,
                }],
            },
        );
    }

    Ok(json!({
        "spec": {
            "format_version": 1.0,
            "suspend_timeout_seconds": -1,
            "cluster": {
                "cluster_id": project.spec.id,
                "name": project.spec.name,
                "roles": [{
                    "name": project.spec.superuser_name,
                    "encrypted_password": "b093c0d3b281ba6da1eacc608620abd8",
                    "options": null
                }],
                "databases": [],
                "settings": settings,
            },
            "delta_operations": [],
            "safekeeper_connstrings": safekeeper_connstrings,
            "pageserver_connection_info": json!({
                "shard_count": 0,
                "shards": shards,
            }),
        },
        "compute_ctl_config": {
            "jwks": jwks
        },
        "status": "attached"
    }))
}

pub async fn find_compute_deployment(client: &kube::Client, compute_id: &str) -> Result<Deployment> {
    let deployments: Api<Deployment> = Api::all(client.clone());
    let deployment_name = format!("{}-compute-node", compute_id);

    info!("Looking for deployment: {}", deployment_name);

    let deps = deployments
        .list(&ListParams {
            label_selector: Some(format!("app={}", deployment_name)),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            error!("Failed to get deployment {}: {}", deployment_name, e);
            Error::StdError(StdError::KubeError(e))
        })?;

    Ok(deps.clone().iter().next().unwrap().clone())
}

pub fn extract_cluster_name(deployment: &Deployment) -> Result<String> {
    deployment
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get("neon.cluster_name"))
        .cloned()
        .ok_or_else(|| {
            Error::StdError(StdError::MetadataMissing(
                "neon.cluster_name annotation not found".into(),
            ))
        })
}

async fn find_project_and_branch(
    client: &kube::Client,
    tenant_id: &str,
    timeline_id: &str,
) -> Result<(NeonProject, NeonBranch)> {
    info!(
        "Searching for project with tenant_id: {} and branch with timeline_id: {}",
        tenant_id, timeline_id
    );

    // Find project by tenant_id
    let projects: Api<NeonProject> = Api::all(client.clone());
    let project_list = projects.list(&Default::default()).await.map_err(|e| {
        error!("Failed to list NeonProject resources: {}", e);
        Error::StdError(StdError::KubeError(e))
    })?;

    let project = project_list
        .items
        .into_iter()
        .find(|p| p.spec.tenant_id.as_ref() == Some(&tenant_id.to_string()))
        .ok_or_else(|| {
            error!("No NeonProject found with tenant_id: {}", tenant_id);
            Error::StdError(StdError::MetadataMissing(format!(
                "Project with tenant_id {} not found",
                tenant_id
            )))
        })?;

    info!(
        "Found project: {}",
        project.metadata.name.as_ref().unwrap_or(&"<unnamed>".to_string())
    );

    // Find branch by timeline_id
    let branches: Api<NeonBranch> = Api::all(client.clone());
    let branch_list = branches.list(&Default::default()).await.map_err(|e| {
        error!("Failed to list NeonBranch resources: {}", e);
        Error::StdError(StdError::KubeError(e))
    })?;

    let branch = branch_list
        .items
        .into_iter()
        .find(|b| b.spec.timeline_id.as_ref() == Some(&timeline_id.to_string()))
        .ok_or_else(|| {
            error!("No NeonBranch found with timeline_id: {}", timeline_id);
            Error::StdError(StdError::MetadataMissing(format!(
                "Branch with timeline_id {} not found",
                timeline_id
            )))
        })?;

    info!(
        "Found branch: {}",
        branch.metadata.name.as_ref().unwrap_or(&"<unnamed>".to_string())
    );

    Ok((project, branch))
}

fn build_postgres_settings(cluster_name: &str, tenant_id: &str, timeline_id: &str) -> Vec<serde_json::Value> {
    let settings = vec![
        json!({"name": "fsync", "value": "off", "vartype": "bool"}),
        json!({"name": "wal_level", "value": "logical", "vartype": "enum"}),
        json!({"name": "wal_log_hints", "value": "on", "vartype": "bool"}),
        json!({"name": "log_connections", "value": "on", "vartype": "bool"}),
        json!({"name": "port", "value": "55433", "vartype": "integer"}),
        json!({"name": "shared_buffers", "value": "1MB", "vartype": "string"}),
        json!({"name": "max_connections", "value": "100", "vartype": "integer"}),
        json!({"name": "listen_addresses", "value": "0.0.0.0", "vartype": "string"}),
        json!({"name": "max_wal_senders", "value": "10", "vartype": "integer"}),
        json!({"name": "max_replication_slots", "value": "10", "vartype": "integer"}),
        json!({"name": "wal_sender_timeout", "value": "5s", "vartype": "string"}),
        json!({"name": "wal_keep_size", "value": "0", "vartype": "integer"}),
        json!({"name": "password_encryption", "value": "md5", "vartype": "enum"}),
        json!({"name": "restart_after_crash", "value": "off", "vartype": "bool"}),
        json!({"name": "synchronous_standby_names", "value": "walproposer", "vartype": "string"}),
        json!({"name": "shared_preload_libraries", "value": "neon", "vartype": "string"}),
        json!({
            "name": "neon.safekeepers",
            "value": format!(
                "safekeeper-{0}-0.neon:5454,safekeeper-{0}-1.neon:5454,safekeeper-{0}-2.neon:5454",
                cluster_name
            ),
            "vartype": "string"
        }),
        json!({"name": "neon.timeline_id", "value": timeline_id, "vartype": "string"}),
        json!({"name": "neon.tenant_id", "value": tenant_id, "vartype": "string"}),
        json!({"name": "max_replication_write_lag", "value": "500MB", "vartype": "string"}),
        json!({"name": "max_replication_flush_lag", "value": "10GB", "vartype": "string"}),
        json!({"name": "neon.max_file_cache_size", "value": "1GB", "vartype": "string"}),
    ];

    settings
}
