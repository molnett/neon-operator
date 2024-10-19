use std::sync::Arc;
use std::time::Duration;

use super::cluster_controller::Context;
use super::resources::{NeonProject, NeonProjectStatus};
use crate::util::errors::{Error, Result, StdError};

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use kube::api::{Api, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::ResourceExt;
use serde_json::json;

pub const COMPUTE_NODE_READY_CONDITION: &str = "ComputeNodeReady";
pub const DEFAULT_USER_CREATED_CONDITION: &str = "DefaultUserCreated";
pub const DEFAULT_DATABASE_CREATED_CONDITION: &str = "DefaultDatabaseCreated";

pub async fn update_status(
    client: &kube::Client,
    namespace: &str,
    name: &str,
    compute_node_ready: bool,
) -> Result<()> {
    let projects: Api<NeonProject> = Api::namespaced(client.clone(), namespace);
    let status = if compute_node_ready {
        NeonProjectStatus {
            conditions: vec![Condition {
                type_: COMPUTE_NODE_READY_CONDITION.to_string(),
                status: compute_node_ready.to_string(),
                last_transition_time: Time(chrono::Utc::now()),
                message: "Compute node is ready".to_string(),
                reason: "ComputeNodeStarted".to_string(),
                observed_generation: None,
            }],
        }
    } else {
        NeonProjectStatus {
            conditions: vec![Condition {
                type_: COMPUTE_NODE_READY_CONDITION.to_string(),
                status: "False".to_string(),
                last_transition_time: Time(chrono::Utc::now()),
                message: "Compute node is not ready".to_string(),
                reason: "ComputeNodeNotReady".to_string(),
                observed_generation: None,
            }],
        }
    };

    let patch = Patch::Merge(json!({
        "status": status
    }));

    projects
        .patch_status(name, &PatchParams::default(), &patch)
        .await
        .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

    Ok(())
}

pub fn is_condition_met(neon_project: &NeonProject, condition_type: &str) -> bool {
    neon_project
        .status
        .as_ref()
        .and_then(|status| Some(status.conditions.as_ref()))
        .map(|conditions: &Vec<Condition>| {
            conditions
                .iter()
                .any(|condition| condition.type_ == condition_type && condition.status == "True")
        })
        .unwrap_or(false)
}
