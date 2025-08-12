use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::v1::{conditions_schema, PGVersion};

pub static NEON_CLUSTER_FINALIZER: &str = "neon-cluster.oltp.molnett.org";

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct StorageConfig {
    /// Storage class to use for persistent volume claims
    pub storage_class: Option<String>,
    /// Size of the persistent volume
    #[serde(default = "default_storage_size")]
    pub size: String,
}

fn default_storage_size() -> String {
    "10Gi".to_string()
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_class: None,
            size: default_storage_size(),
        }
    }
}

/// Generate the Kubernetes wrapper struct `NeonCluster` from our Spec and Status struct
///
/// This provides a hook for generating the CRD yaml (in crdgen.rs)
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "NeonCluster", group = "oltp.molnett.org", version = "v1", namespaced)]
#[kube(status = "NeonClusterStatus", shortname = "neoncluster")]
pub struct NeonClusterSpec {
    #[serde(default = "default_num_safekeepers")]
    pub num_safekeepers: u8,
    #[serde(default = "default_pg_version")]
    pub default_pg_version: PGVersion,
    #[serde(default = "default_neon_image")]
    pub neon_image: String,

    pub bucket_credentials_secret: String,
    pub storage_controller_database_url: String,

    /// Storage configuration for safekeeper persistent volumes
    #[serde(default)]
    pub safekeeper_storage: StorageConfig,
}

fn default_num_safekeepers() -> u8 {
    3
}
fn default_pg_version() -> PGVersion {
    PGVersion::PG16
}
fn default_neon_image() -> String {
    "neondatabase/neon:6351-bookworm".to_string()
}

/// The status object of `NeonCluster`
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterStatus {
    #[schemars(schema_with = "conditions_schema")]
    pub conditions: Vec<Condition>,
    pub phase: Option<String>,
    pub storage_broker_status: NeonClusterStorageBrokerStatus,
    pub safekeeper_status: NeonClusterSafeKeeperStatus,
}

/// The status object of `NeonCluster` StorageBroker component
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterStorageBrokerStatus {
    pub ready: bool,
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,
}

/// The status object of `NeonCluster` SafeKeeper component
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterSafeKeeperStatus {
    pub ready: bool,
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,
}
