use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use kube::api::{Api, Patch, PatchParams};
use kube::ResourceExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use tracing::info;

use crate::api::v1::neonproject::NeonProject;
use crate::util::errors::{Error, Result, StdError};
use crate::util::status::set_status_condition;

// Constants for condition types
pub const PROJECT_READY_CONDITION: &str = "Ready";
pub const TENANT_CREATED_CONDITION: &str = "TenantCreated";
pub const PAGESERVER_CONFIGURED_CONDITION: &str = "PageServerConfigured";

// Field manager for status updates - must match the project controller's field manager
pub const STATUS_FIELD_MANAGER: &str = "neon-project-controller";

// Phase represents the high-level status of a NeonProject
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ProjectPhase {
    Pending,
    Creating,
    Ready,
    Failed,
    Terminating,
}

impl fmt::Display for ProjectPhase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProjectPhase::Pending => write!(f, "Pending"),
            ProjectPhase::Creating => write!(f, "Creating"),
            ProjectPhase::Ready => write!(f, "Ready"),
            ProjectPhase::Failed => write!(f, "Failed"),
            ProjectPhase::Terminating => write!(f, "Terminating"),
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

    // Project-specific reasons
    TenantCreating,
    TenantCreated,
    TenantFailed,
    PageServerUnavailable,
    PageServerConfigured,
}

impl fmt::Display for StatusReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StatusReason::Pending => write!(f, "Pending"),
            StatusReason::InProgress => write!(f, "InProgress"),
            StatusReason::Completed => write!(f, "Completed"),
            StatusReason::Failed => write!(f, "Failed"),
            StatusReason::TenantCreating => write!(f, "TenantCreating"),
            StatusReason::TenantCreated => write!(f, "TenantCreated"),
            StatusReason::TenantFailed => write!(f, "TenantFailed"),
            StatusReason::PageServerUnavailable => write!(f, "PageServerUnavailable"),
            StatusReason::PageServerConfigured => write!(f, "PageServerConfigured"),
        }
    }
}

pub struct ProjectStatusManager<'a> {
    project: &'a NeonProject,
    client: kube::Client,
}

impl<'a> ProjectStatusManager<'a> {
    pub fn new(client: &kube::Client, project: &'a NeonProject) -> Result<Self> {
        Ok(Self {
            project,
            client: client.clone(),
        })
    }

    /// Updates the phase of the project
    pub async fn update_phase(&self, phase: ProjectPhase) -> Result<()> {
        let name = self.project.name_any();
        let namespace = self.project.namespace().unwrap();
        let api: Api<NeonProject> = Api::namespaced(self.client.clone(), &namespace);

        // Get current status to preserve existing conditions
        let current_project = api
            .get(&name)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let current_conditions = current_project
            .status
            .as_ref()
            .map_or_else(Vec::new, |s| s.conditions.clone());

        let patch = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1",
            "kind": "NeonProject",
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

        info!("Updated project {} phase to {}", name, phase);
        Ok(())
    }

    /// Sets the project ready condition
    pub async fn set_project_ready(&self, ready: bool) -> Result<()> {
        let (status, reason, message) = if ready {
            ("True", StatusReason::Completed, "Project is ready and configured")
        } else {
            ("False", StatusReason::InProgress, "Project is not ready")
        };

        self.set_condition(PROJECT_READY_CONDITION, status, reason, message)
            .await
    }

    /// Sets the tenant created condition
    pub async fn set_tenant_created(&self, created: bool) -> Result<()> {
        let (status, reason, message) = if created {
            ("True", StatusReason::TenantCreated, "Tenant created successfully")
        } else {
            (
                "False",
                StatusReason::TenantCreating,
                "Tenant creation in progress",
            )
        };

        self.set_condition(TENANT_CREATED_CONDITION, status, reason, message)
            .await
    }

    /// Sets the tenant creation failed condition
    pub async fn set_tenant_failed(&self, error_message: &str) -> Result<()> {
        self.set_condition(
            TENANT_CREATED_CONDITION,
            "False",
            StatusReason::TenantFailed,
            error_message,
        )
        .await
    }

    /// Sets the pageserver configured condition
    pub async fn set_pageserver_configured(&self, configured: bool) -> Result<()> {
        let (status, reason, message) = if configured {
            (
                "True",
                StatusReason::PageServerConfigured,
                "PageServer tenant configured successfully",
            )
        } else {
            (
                "False",
                StatusReason::PageServerUnavailable,
                "PageServer not available or configuration failed",
            )
        };

        self.set_condition(PAGESERVER_CONFIGURED_CONDITION, status, reason, message)
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
        let name = self.project.name_any();
        let namespace = self.project.namespace().unwrap();
        let api: Api<NeonProject> = Api::namespaced(self.client.clone(), &namespace);

        // Get current status
        let current_project = api
            .get(&name)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

        let current_conditions = current_project
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
            observed_generation: current_project.metadata.generation,
        };

        // Update conditions
        let (new_conditions, _changed) = set_status_condition(&current_conditions, new_condition);

        // Preserve existing phase
        let current_phase = current_project.status.as_ref().and_then(|s| s.phase.clone());

        let patch = Patch::Apply(json!({
            "apiVersion": "oltp.molnett.org/v1",
            "kind": "NeonProject",
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

        info!(
            "Updated project {} condition {} to {}",
            name, condition_type, status
        );
        Ok(())
    }
}
