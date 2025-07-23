use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use kube::api::{Api, Patch, PatchParams};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use tracing::info;

use crate::controllers::resources::NeonBranch;
use crate::util::errors::{Error, Result, StdError};
use crate::util::status::set_status_condition;

// Constants for condition types
pub const COMPUTE_NODE_READY_CONDITION: &str = "ComputeNodeReady";
pub const DEFAULT_USER_CREATED_CONDITION: &str = "DefaultUserCreated";
pub const DEFAULT_DATABASE_CREATED_CONDITION: &str = "DefaultDatabaseCreated";

// Field manager for status updates - must match the branch controller's field manager
pub const STATUS_FIELD_MANAGER: &str = "neon-branch-controller";

// Phase represents the high-level status of a NeonBranch
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BranchPhase {
    Pending,
    Creating,
    Ready,
    Failed,
    Terminating,
}

impl fmt::Display for BranchPhase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BranchPhase::Pending => write!(f, "Pending"),
            BranchPhase::Creating => write!(f, "Creating"),
            BranchPhase::Ready => write!(f, "Ready"),
            BranchPhase::Failed => write!(f, "Failed"),
            BranchPhase::Terminating => write!(f, "Terminating"),
        }
    }
}

// Status reasons for conditions
#[derive(Debug, Clone, PartialEq)]
pub enum StatusReason {
    // Generic reasons
    Pending,
    InProgress,
    Completed,
    Failed,

    // Specific reasons
    ProjectNotFound,
    TimelineCreated,
    ComputeNodeStarted,
    ComputeNodeNotReady,
    DefaultUserCreated,
    DefaultDatabaseCreated,
    CustomReason(String),
}

impl fmt::Display for StatusReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StatusReason::Pending => write!(f, "Pending"),
            StatusReason::InProgress => write!(f, "InProgress"),
            StatusReason::Completed => write!(f, "Completed"),
            StatusReason::Failed => write!(f, "Failed"),
            StatusReason::ProjectNotFound => write!(f, "ProjectNotFound"),
            StatusReason::TimelineCreated => write!(f, "TimelineCreated"),
            StatusReason::ComputeNodeStarted => write!(f, "ComputeNodeStarted"),
            StatusReason::ComputeNodeNotReady => write!(f, "ComputeNodeNotReady"),
            StatusReason::DefaultUserCreated => write!(f, "DefaultUserCreated"),
            StatusReason::DefaultDatabaseCreated => write!(f, "DefaultDatabaseCreated"),
            StatusReason::CustomReason(s) => write!(f, "{}", s),
        }
    }
}

// Status messages for conditions
#[derive(Debug, Clone, PartialEq)]
pub enum StatusMessage {
    // Generic messages
    Pending,
    InProgress,
    Completed,
    Failed(String),

    // Specific messages
    ProjectNotFound,
    TimelineCreated,
    ComputeNodeReady,
    ComputeNodeNotReady,
    DefaultUserCreated,
    DefaultDatabaseCreated,
    CustomMessage(String),
}

impl fmt::Display for StatusMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StatusMessage::Pending => write!(f, "Pending"),
            StatusMessage::InProgress => write!(f, "In progress"),
            StatusMessage::Completed => write!(f, "Completed"),
            StatusMessage::Failed(s) => write!(f, "Failed: {}", s),
            StatusMessage::ProjectNotFound => write!(f, "Project not found"),
            StatusMessage::TimelineCreated => write!(f, "Timeline successfully created"),
            StatusMessage::ComputeNodeReady => write!(f, "Compute node is ready"),
            StatusMessage::ComputeNodeNotReady => write!(f, "Compute node is not ready"),
            StatusMessage::DefaultUserCreated => write!(f, "Default user created"),
            StatusMessage::DefaultDatabaseCreated => write!(f, "Default database created"),
            StatusMessage::CustomMessage(s) => write!(f, "{}", s),
        }
    }
}

// Status manager for NeonBranch resources
pub struct BranchStatusManager<'a> {
    client: &'a kube::Client,
    namespace: String,
    name: String,
}

impl<'a> BranchStatusManager<'a> {
    pub fn new(client: &'a kube::Client, branch: &'a NeonBranch) -> Result<Self> {
        let namespace = branch.metadata.namespace.clone().ok_or_else(|| {
            Error::StdError(StdError::MetadataMissing(
                "Branch resource has no namespace".to_string(),
            ))
        })?;
        let name = branch.metadata.name.clone().unwrap_or_default();

        Ok(Self {
            client,
            namespace,
            name,
        })
    }

    // Create a new condition
    fn create_condition(
        &self,
        condition_type: &str,
        status: bool,
        reason: StatusReason,
        message: StatusMessage,
        observed_generation: Option<i64>,
    ) -> Condition {
        Condition {
            type_: condition_type.to_string(),
            status: status.to_string(),
            last_transition_time: Time(chrono::Utc::now()),
            reason: reason.to_string(),
            message: message.to_string(),
            observed_generation,
        }
    }

    // Update a specific condition
    pub async fn update_condition(
        &self,
        condition_type: &str,
        status: bool,
        reason: StatusReason,
        message: StatusMessage,
    ) -> Result<()> {
        // Get current branch to access metadata.generation
        let branch_client: Api<NeonBranch> = Api::namespaced(self.client.clone(), &self.namespace);
        let current_branch = branch_client
            .get(&self.name)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let condition = self.create_condition(
            condition_type,
            status,
            reason,
            message,
            current_branch.metadata.generation,
        );
        let current_conditions = current_branch
            .status
            .as_ref()
            .map(|s| s.conditions.clone())
            .unwrap_or_default();

        let (updated_conditions, changed) = set_status_condition(&current_conditions, condition);

        if changed {
            let branch_client: Api<NeonBranch> = Api::namespaced(self.client.clone(), &self.namespace);

            // Get current phase from the fresh object or default to Pending
            let phase = match (condition_type, status) {
                (COMPUTE_NODE_READY_CONDITION, true) => BranchPhase::Ready,
                _ => current_branch
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_ref())
                    .and_then(|p| serde_json::from_str(p).ok())
                    .unwrap_or(BranchPhase::Pending),
            };

            // Create patch that preserves other status fields
            let patch = Patch::Merge(json!({
                "status": {
                    "conditions": updated_conditions,
                    "phase": phase.to_string()
                }
            }));

            branch_client
                .patch_status(&self.name, &PatchParams::apply(STATUS_FIELD_MANAGER), &patch)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

            info!(
                "Updated condition {} to {}",
                condition_type,
                if status { "True" } else { "False" }
            );
        }

        Ok(())
    }

    // Set compute node readiness
    pub async fn set_compute_node_ready(&self, ready: bool) -> Result<()> {
        let (reason, message) = if ready {
            (StatusReason::ComputeNodeStarted, StatusMessage::ComputeNodeReady)
        } else {
            (
                StatusReason::ComputeNodeNotReady,
                StatusMessage::ComputeNodeNotReady,
            )
        };

        self.update_condition(COMPUTE_NODE_READY_CONDITION, ready, reason, message)
            .await
    }

    // Mark default user as created
    pub async fn set_default_user_created(&self) -> Result<()> {
        self.update_condition(
            DEFAULT_USER_CREATED_CONDITION,
            true,
            StatusReason::DefaultUserCreated,
            StatusMessage::DefaultUserCreated,
        )
        .await
    }

    // Mark default database as created
    pub async fn set_default_database_created(&self) -> Result<()> {
        self.update_condition(
            DEFAULT_DATABASE_CREATED_CONDITION,
            true,
            StatusReason::DefaultDatabaseCreated,
            StatusMessage::DefaultDatabaseCreated,
        )
        .await
    }

    // Update branch phase
    pub async fn update_phase(&self, phase: BranchPhase) -> Result<()> {
        // Get current branch from API server
        let branch_client: Api<NeonBranch> = Api::namespaced(self.client.clone(), &self.namespace);
        let current_branch = branch_client
            .get(&self.name)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        // Get current phase from the fresh object
        let current_phase = current_branch
            .status
            .as_ref()
            .and_then(|s| s.phase.as_ref())
            .map(|p| p.as_str());

        let target_phase = phase.to_string();

        // Only update if phase actually changed (or if no phase was set before)
        let should_update = match current_phase {
            Some(current) => current != target_phase,
            None => true, // Only update once when transitioning from None to a value
        };

        if should_update {
            let branch_client: Api<NeonBranch> = Api::namespaced(self.client.clone(), &self.namespace);

            let patch = Patch::Merge(json!({
                "status": {
                    "phase": phase.to_string()
                }
            }));

            branch_client
                .patch_status(&self.name, &PatchParams::apply(STATUS_FIELD_MANAGER), &patch)
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

            info!("Updated branch phase to {}", phase);
        }

        Ok(())
    }
}
