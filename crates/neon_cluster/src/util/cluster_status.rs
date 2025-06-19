use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use kube::api::{Api, Patch, PatchParams};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use tracing::info;

use crate::controllers::resources::NeonCluster;
use crate::util::errors::{Error, Result, StdError};
use crate::util::status::set_status_condition;

// Constants for condition types
pub const CLUSTER_READY_CONDITION: &str = "Ready";
pub const PAGESERVER_READY_CONDITION: &str = "PageServerReady";
pub const SAFEKEEPER_READY_CONDITION: &str = "SafeKeeperReady";
pub const STORAGE_BROKER_READY_CONDITION: &str = "StorageBrokerReady";

// Field manager for status updates
pub const STATUS_FIELD_MANAGER: &str = "neon-cluster-status-manager";

// Phase represents the high-level status of a NeonCluster
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ClusterPhase {
    Pending,
    Creating,
    Ready,
    Failed,
    Terminating,
}

impl fmt::Display for ClusterPhase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ClusterPhase::Pending => write!(f, "Pending"),
            ClusterPhase::Creating => write!(f, "Creating"),
            ClusterPhase::Ready => write!(f, "Ready"),
            ClusterPhase::Failed => write!(f, "Failed"),
            ClusterPhase::Terminating => write!(f, "Terminating"),
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
    
    // Cluster-specific reasons
    ComponentsCreating,
    ComponentsReady,
    ComponentsFailed,
}

impl fmt::Display for StatusReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StatusReason::Pending => write!(f, "Pending"),
            StatusReason::InProgress => write!(f, "InProgress"),
            StatusReason::Completed => write!(f, "Completed"),
            StatusReason::Failed => write!(f, "Failed"),
            StatusReason::ComponentsCreating => write!(f, "ComponentsCreating"),
            StatusReason::ComponentsReady => write!(f, "ComponentsReady"),
            StatusReason::ComponentsFailed => write!(f, "ComponentsFailed"),
        }
    }
}

pub struct ClusterStatusManager<'a> {
    cluster: &'a NeonCluster,
    client: kube::Client,
}

impl<'a> ClusterStatusManager<'a> {
    pub fn new(client: &kube::Client, cluster: &'a NeonCluster) -> Result<Self> {
        Ok(Self {
            cluster,
            client: client.clone(),
        })
    }

    /// Updates the phase of the cluster
    pub async fn update_phase(&self, phase: ClusterPhase) -> Result<()> {
        let name = self.cluster.name_any();
        let namespace = self.cluster.namespace().unwrap();
        let api: Api<NeonCluster> = Api::namespaced(self.client.clone(), &namespace);

        // Get current status to preserve existing conditions
        let current_cluster = api.get(&name).await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let current_conditions = current_cluster.status
            .as_ref()
            .map_or_else(Vec::new, |s| s.conditions.clone());

        let patch = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1",
            "kind": "NeonCluster",
            "metadata": {
                "name": name,
                "namespace": namespace
            },
            "status": {
                "phase": phase.to_string(),
                "conditions": current_conditions
            }
        }));

        let patch_params = PatchParams::apply(STATUS_FIELD_MANAGER);
        
        api.patch_status(&name, &patch_params, &patch)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        info!("Updated cluster {} phase to {}", name, phase);
        Ok(())
    }

    /// Sets the cluster ready condition
    pub async fn set_cluster_ready(&self, ready: bool) -> Result<()> {
        let (status, reason, message) = if ready {
            ("True", StatusReason::ComponentsReady, "All cluster components are ready")
        } else {
            ("False", StatusReason::ComponentsCreating, "Cluster components are not ready")
        };

        self.set_condition(
            CLUSTER_READY_CONDITION,
            status,
            reason,
            message,
        ).await
    }

    /// Sets the pageserver ready condition
    pub async fn set_pageserver_ready(&self, ready: bool) -> Result<()> {
        let (status, reason, message) = if ready {
            ("True", StatusReason::Completed, "PageServer is ready")
        } else {
            ("False", StatusReason::InProgress, "PageServer is not ready")
        };

        self.set_condition(
            PAGESERVER_READY_CONDITION,
            status,
            reason,
            message,
        ).await
    }

    /// Sets the safekeeper ready condition
    pub async fn set_safekeeper_ready(&self, ready: bool) -> Result<()> {
        let (status, reason, message) = if ready {
            ("True", StatusReason::Completed, "SafeKeeper is ready")
        } else {
            ("False", StatusReason::InProgress, "SafeKeeper is not ready")
        };

        self.set_condition(
            SAFEKEEPER_READY_CONDITION,
            status,
            reason,
            message,
        ).await
    }

    /// Sets the storage broker ready condition
    pub async fn set_storage_broker_ready(&self, ready: bool) -> Result<()> {
        let (status, reason, message) = if ready {
            ("True", StatusReason::Completed, "StorageBroker is ready")
        } else {
            ("False", StatusReason::InProgress, "StorageBroker is not ready")
        };

        self.set_condition(
            STORAGE_BROKER_READY_CONDITION,
            status,
            reason,
            message,
        ).await
    }

    /// Helper method to set a condition
    async fn set_condition(
        &self,
        condition_type: &str,
        status: &str,
        reason: StatusReason,
        message: &str,
    ) -> Result<()> {
        let name = self.cluster.name_any();
        let namespace = self.cluster.namespace().unwrap();
        let api: Api<NeonCluster> = Api::namespaced(self.client.clone(), &namespace);

        // Get current status
        let current_cluster = api.get(&name).await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let current_conditions = current_cluster.status
            .as_ref()
            .map_or_else(Vec::new, |s| s.conditions.clone());

        // Create new condition
        let new_condition = Condition {
            type_: condition_type.to_string(),
            status: status.to_string(),
            reason: reason.to_string(),
            message: message.to_string(),
            last_transition_time: Time(chrono::Utc::now()),
            observed_generation: current_cluster.metadata.generation,
        };

        // Update conditions
        let (new_conditions, _changed) = set_status_condition(&current_conditions, new_condition);

        // Preserve existing phase
        let current_phase = current_cluster.status
            .as_ref()
            .and_then(|s| s.phase.clone());

        let patch = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1",
            "kind": "NeonCluster",
            "metadata": {
                "name": name,
                "namespace": namespace
            },
            "status": {
                "conditions": new_conditions,
                "phase": current_phase
            }
        }));

        let patch_params = PatchParams::apply(STATUS_FIELD_MANAGER);
        
        api.patch_status(&name, &patch_params, &patch)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        info!("Updated cluster {} condition {} to {}", name, condition_type, status);
        Ok(())
    }
}