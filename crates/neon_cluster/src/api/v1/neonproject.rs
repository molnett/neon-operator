use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::v1::{conditions_schema, PGVersion};

pub static NEON_PROJECT_FINALIZER: &str = "neon-project.oltp.molnett.org";

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
