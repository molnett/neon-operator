use crate::api::v1::{neoncluster::StorageConfig, NodeId};

use crate::api::v1::conditions_schema;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub static NEON_PAGESERVER_FINALIZER: &str = "neon-pageserver.oltp.molnett.org";

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[cfg_attr(test, derive(Default))]
#[kube(
    kind = "NeonPageserver",
    group = "oltp.molnett.org",
    version = "v1alpha1",
    namespaced
)]
#[kube(status = "NeonPageserverStatus", shortname = "neonpageserver")]
pub struct NeonPageserverSpec {
    pub id: NodeId,

    pub cluster: String,
    pub bucket_credentials_secret: String,

    #[serde(default)]
    pub storage_config: StorageConfig,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct NeonPageserverStatus {
    #[schemars(schema_with = "conditions_schema")]
    pub conditions: Vec<Condition>,
    pub phase: Option<String>,
}
