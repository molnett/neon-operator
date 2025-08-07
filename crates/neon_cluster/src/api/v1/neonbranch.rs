use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::v1::{conditions_schema, PGVersion};

pub static NEON_BRANCH_FINALIZER: &str = "neon-branch.oltp.molnett.org";

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
