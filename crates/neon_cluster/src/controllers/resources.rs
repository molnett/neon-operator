use core::fmt;
use std::fmt::Display;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub static NEON_CLUSTER_FINALIZER: &str = "neon-cluster.oltp.molnett.org";
pub static NEON_PROJECT_FINALIZER: &str = "neon-project.oltp.molnett.org";
pub static NEON_BRANCH_FINALIZER: &str = "neon-branch.oltp.molnett.org";

#[derive(Default, Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub enum PGVersion {
    PG14 = 14,
    #[default]
    PG15 = 15,
    PG16 = 16,
}

impl Display for PGVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.clone() as isize)
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
    #[serde(default = "default_num_pageservers")]
    pub num_pageservers: i32,
    #[serde(default = "default_pg_version")]
    pub default_pg_version: PGVersion,
    #[serde(default = "default_neon_image")]
    pub neon_image: String,

    pub bucket_credentials_secret: String,
    pub storage_controller_database_url: String,
}

fn default_num_safekeepers() -> u8 {
    3
}
fn default_num_pageservers() -> i32 {
    1
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
    pub page_server_status: NeonClusterPageServerStatus,
    pub storage_broker_status: NeonClusterStorageBrokerStatus,
    pub safekeeper_status: NeonClusterSafeKeeperStatus,
}

/// The status object of `NeonCluster` PageServer component
#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonClusterPageServerStatus {
    pub ready: bool,
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,
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

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "NeonProject", group = "oltp.molnett.org", version = "v1", namespaced)]
#[kube(status = "NeonProjectStatus", shortname = "neonproject")]
pub struct NeonProjectSpec {
    pub cluster_name: String,

    pub id: String,
    pub name: String,

    // 32 character alphanumeric string
    pub tenant_id: Option<String>,

    pub pg_version: PGVersion,
    #[serde(default = "default_compute_size")]
    pub default_compute_size: f32,
    pub default_database_name: String,
    pub superuser_name: String,
}

fn default_compute_size() -> f32 {
    0.25
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonProjectStatus {
    #[schemars(schema_with = "conditions_schema")]
    pub conditions: Vec<Condition>,
    pub phase: Option<String>,
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(kind = "NeonBranch", group = "oltp.molnett.org", version = "v1", namespaced)]
#[kube(status = "NeonBranchStatus", shortname = "neonbranch")]
pub struct NeonBranchSpec {
    pub id: String,
    pub name: String,

    // 32 character alphanumeric string
    pub timeline_id: Option<String>,

    pub pg_version: PGVersion,
    pub default_branch: bool,
    pub project_id: String,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonBranchStatus {
    #[schemars(schema_with = "conditions_schema")]
    pub conditions: Vec<Condition>,
    pub phase: Option<String>,
}

fn conditions_schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    serde_json::from_value(serde_json::json!({
        "type": "array",
        "x-kubernetes-list-type": "map",
        "x-kubernetes-list-map-keys": ["type"],
        "items": {
            "type": "object",
            "properties": {
                "lastTransitionTime": { "format": "date-time", "type": "string" },
                "message": { "type": "string" },
                "observedGeneration": { "type": "integer", "format": "int64", "default": 0 },
                "reason": { "type": "string" },
                "status": { "type": "string" },
                "type": { "type": "string" }
            },
            "required": [
                "lastTransitionTime",
                "message",
                "reason",
                "status",
                "type"
            ],
        },
    }))
    .unwrap()
}
