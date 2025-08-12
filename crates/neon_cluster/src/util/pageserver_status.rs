use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use kube::api::{Api, Patch, PatchParams};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use tracing::info;

use crate::api::v1alpha1::neonpageserver::NeonPageserver;
use crate::controllers::pageserver_controller::FIELD_MANAGER;
use crate::util::errors::{Error, Result, StdError};
use crate::util::status::set_status_condition;

// Constants for condition types
pub const PAGESERVER_READY_CONDITION: &str = "Ready";

// Phase represents the high-level status of a NeonPageserver
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PageserverPhase {
    Pending,
    Creating,
    Ready,
    Failed,
    Terminating,
}

impl fmt::Display for PageserverPhase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PageserverPhase::Pending => write!(f, "Pending"),
            PageserverPhase::Creating => write!(f, "Creating"),
            PageserverPhase::Ready => write!(f, "Ready"),
            PageserverPhase::Failed => write!(f, "Failed"),
            PageserverPhase::Terminating => write!(f, "Terminating"),
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
}

impl fmt::Display for StatusReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StatusReason::Pending => write!(f, "Pending"),
            StatusReason::InProgress => write!(f, "InProgress"),
            StatusReason::Completed => write!(f, "Completed"),
            StatusReason::Failed => write!(f, "Failed"),
        }
    }
}

pub struct PageserverStatusManager<'a> {
    pageserver: &'a NeonPageserver,
    client: kube::Client,
}

impl<'a> PageserverStatusManager<'a> {
    pub fn new(client: &kube::Client, pageserver: &'a NeonPageserver) -> Result<Self> {
        Ok(Self {
            pageserver,
            client: client.clone(),
        })
    }

    /// Updates the phase of the pageserver
    pub async fn update_phase(&self, phase: PageserverPhase) -> Result<()> {
        let name = self.pageserver.name_any();
        let namespace = self.pageserver.namespace().unwrap();
        let api: Api<NeonPageserver> = Api::namespaced(self.client.clone(), &namespace);

        // Get current status to preserve existing conditions
        let current_pageserver = api
            .get(&name)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let current_conditions = current_pageserver
            .status
            .as_ref()
            .map_or_else(Vec::new, |s| s.conditions.clone());

        let patch = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1alpha1",
            "kind": "NeonPageserver",
            "metadata": {
                "name": name,
                "namespace": namespace
            },
            "status": {
                "phase": phase.to_string(),
                "conditions": current_conditions
            }
        }));

        let patch_params = PatchParams::apply(FIELD_MANAGER);

        api.patch_status(&name, &patch_params, &patch)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        info!("Updated pageserver {} phase to {}", name, phase);
        Ok(())
    }

    /// Sets the pageserver ready condition
    pub async fn set_pageserver_ready(&self, ready: bool) -> Result<()> {
        let (status, reason, message) = if ready {
            ("True", StatusReason::Completed, "PageServer is ready")
        } else {
            ("False", StatusReason::InProgress, "PageServer is not ready")
        };

        self.set_condition(PAGESERVER_READY_CONDITION, status, reason, message)
            .await
    }

    /// Helper method to set a condition
    async fn set_condition(
        &self,
        condition_type: &str,
        status: &str,
        reason: StatusReason,
        message: &str,
    ) -> Result<()> {
        let name = self.pageserver.name_any();
        let namespace = self.pageserver.namespace().unwrap();
        let api: Api<NeonPageserver> = Api::namespaced(self.client.clone(), &namespace);

        // Get current status
        let current_pageserver = api
            .get(&name)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let current_conditions = current_pageserver
            .status
            .as_ref()
            .map_or_else(Vec::new, |s| s.conditions.clone());

        // Create new condition
        let new_condition = Condition {
            type_: condition_type.to_string(),
            status: status.to_string(),
            reason: reason.to_string(),
            message: message.to_string(),
            last_transition_time: Time(chrono::Utc::now()),
            observed_generation: current_pageserver.metadata.generation,
        };

        // Update conditions
        let (new_conditions, _changed) = set_status_condition(&current_conditions, new_condition);

        // Preserve existing phase
        let current_phase = current_pageserver.status.as_ref().and_then(|s| s.phase.clone());

        let patch = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1alpha1",
            "kind": "NeonPageserver",
            "metadata": {
                "name": name,
                "namespace": namespace
            },
            "status": {
                "conditions": new_conditions,
                "phase": current_phase
            }
        }));

        let patch_params = PatchParams::apply(FIELD_MANAGER);

        api.patch_status(&name, &patch_params, &patch)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        info!(
            "Updated pageserver {} condition {} to {}",
            name, condition_type, status
        );
        Ok(())
    }
}
