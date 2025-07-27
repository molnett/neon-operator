use super::branch::{
    create_default_database, ensure_deployment, get_or_create_default_user, is_compute_node_ready,
};
use super::resources::*;
use crate::util::branch_status::{
    BranchPhase, BranchStatusManager, DEFAULT_DATABASE_CREATED_CONDITION, DEFAULT_USER_CREATED_CONDITION,
};
use crate::util::errors::{Error, StdError};
use crate::util::status::{is_status_condition_false, is_status_condition_true};
use crate::util::{errors, errors::Result, metrics, telemetry};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use k8s_openapi::api::core::v1::ObjectReference;
use k8s_openapi::api::{apps::v1::Deployment, core::v1::Service};

use kube::{
    api::{Api, ListParams, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{finalizer, Event as Finalizer},
        watcher::{self, Config},
    },
    Resource,
};
use rand::RngCore;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use tracing::*;

pub const FIELD_MANAGER: &str = "neon-branch-controller";

// Use status manager constants for status-related operations

impl NeonBranch {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action, errors::Error> {
        let client = ctx.client.clone();
        let namespace = self.namespace().unwrap();
        let name = self.name_any();

        let branch_client: Api<NeonBranch> = Api::namespaced(client.clone(), &namespace);
        let project_client: Api<NeonProject> = Api::namespaced(client.clone(), &namespace);
        let project = match project_client
            .get_opt(&self.spec.project_id)
            .await
            .map_err(|e| Error::StdError(StdError::KubeError(e)))?
        {
            Some(project) => project,
            None => {
                // Check current status to avoid unnecessary updates
                tracing::info!("Project not found {}", self.spec.project_id);
                let conditions = self
                    .status
                    .as_ref()
                    .map_or_else(Vec::new, |s| s.conditions.clone());
                let current_phase = self.status.as_ref().and_then(|s| s.phase.as_ref());

                let compute_ready_false = is_status_condition_false(&conditions, "ComputeNodeReady");
                let phase_is_failed = current_phase.is_some_and(|p| p == "Failed");

                // Only update status if not already in the correct state
                if !compute_ready_false || !phase_is_failed {
                    let status_manager = BranchStatusManager::new(&client, self)?;
                    status_manager.set_compute_node_ready(false).await?;
                    status_manager.update_phase(BranchPhase::Failed).await?;
                }
                return Ok(Action::requeue(Duration::from_secs(15)));
            }
        };

        if self.spec.timeline_id.is_none() {
            // Set field and set field manager for this field
            // timeline_id is a 32 character alphanumeric string
            let mut bytes = [0; 16];
            rand::thread_rng().fill_bytes(&mut bytes);
            let timeline_id = hex::encode(bytes);
            branch_client
                .patch(
                    &name,
                    &PatchParams {
                        field_manager: Some("neon-operator".to_string()),
                        ..Default::default()
                    },
                    &Patch::Merge(json!({
                        "spec": {
                            "timeline_id": timeline_id
                        }
                    })),
                )
                .await
                .map_err(|e| Error::StdError(StdError::KubeError(e)))?;

            return Ok(Action::requeue(Duration::from_secs(1)));
        }

        // send http request to storage-controller to ensure timeline is created
        let pageserver_url = format!(
            "http://storage-controller-{}:8080/v1/tenant/{}/timeline",
            project.spec.cluster_name,
            project.spec.tenant_id.clone().unwrap()
        );
        tracing::info!("Sending request to pageserver: {}", pageserver_url);
        let http_client = reqwest::Client::new();

        match http_client
            .post(&pageserver_url)
            .header("Content-Type", "application/json")
            .body(format!(
                r#"{{"new_timeline_id": "{}", "pg_version": {}}}"#,
                self.spec.timeline_id.clone().unwrap(),
                self.spec.pg_version
            ))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(response) => {
                if !response.status().is_success() && response.status() != 409 {
                    tracing::warn!("Failed to create timeline on pageserver: {:?}", response.status());
                    return Ok(Action::requeue(Duration::from_secs(5)));
                }
                tracing::info!("Successfully created timeline on pageserver");
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to connect to pageserver at {}: {}. Will retry in 10 seconds.",
                    pageserver_url,
                    e
                );
                return Ok(Action::requeue(Duration::from_secs(10)));
            }
        }

        // Ensure Deployment exists
        ensure_deployment(&client, &namespace, &name, self, &project).await?;

        // Check if Compute node is ready
        let compute_node_ready = is_compute_node_ready(&client, &namespace, &name).await?;

        // Update status only if it has changed
        let conditions = self
            .status
            .as_ref()
            .map_or_else(Vec::new, |s| s.conditions.clone());
        let current_compute_ready = is_status_condition_true(&conditions, "ComputeNodeReady");

        if current_compute_ready != compute_node_ready {
            let status_manager = BranchStatusManager::new(&client, self)?;
            status_manager.set_compute_node_ready(compute_node_ready).await?;

            let phase = if compute_node_ready {
                BranchPhase::Ready
            } else {
                BranchPhase::Pending
            };
            status_manager.update_phase(phase).await?;
        }

        if compute_node_ready {
            // Get conditions safely or use an empty vec
            let conditions = self
                .status
                .as_ref()
                .map_or_else(Vec::new, |s| s.conditions.clone());

            // Create default user and database if not already created
            if !is_status_condition_true(&conditions, DEFAULT_USER_CREATED_CONDITION) {
                get_or_create_default_user(&client, &namespace, &name, self).await?;
            }

            if !is_status_condition_true(&conditions, DEFAULT_DATABASE_CREATED_CONDITION) {
                create_default_database(&client, &namespace, &name, self).await?;
            }
        }

        Ok(Action::requeue(Duration::from_secs(60)))
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>, oref: &ObjectReference) -> Result<Action> {
        let recorder = ctx.diagnostics.read().await.recorder(ctx.client.clone());
        // Branch doesn't have any real cleanup, so we just publish an event
        recorder
            .publish(
                &Event {
                    type_: EventType::Normal,
                    reason: "DeleteRequested".into(),
                    note: Some(format!("Delete `{}`", self.name_any())),
                    action: "Deleting".into(),
                    secondary: None,
                },
                oref,
            )
            .await
            .map_err(|e| errors::Error::StdError(errors::StdError::KubeError(e)))?;
        Ok(Action::await_change())
    }
}

/// State shared between the controller and the web server
#[derive(Clone, Default)]
pub struct State {
    /// Diagnostics populated by the reconciler
    diagnostics: Arc<RwLock<Diagnostics>>,
    /// Metrics registry
    registry: prometheus::Registry,
}

/// State wrapper around the controller outputs for the web server
impl State {
    /// Metrics getter
    pub fn metrics(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }

    /// State getter
    pub async fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.read().await.clone()
    }

    // Create a Controller Context that can update State
    pub fn to_context(&self, client: Client) -> Arc<Context> {
        Arc::new(Context {
            client,
            metrics: metrics::Metrics::default().register(&self.registry).unwrap(),
            diagnostics: self.diagnostics.clone(),
        })
    }
}

// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
    /// Diagnostics read by the web server
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    /// Prometheus metrics
    pub metrics: metrics::Metrics,
}

#[instrument(skip(ctx, neon_branch), fields(trace_id))]
pub async fn reconcile(neon_branch: Arc<NeonBranch>, ctx: Arc<Context>) -> Result<Action> {
    let trace_id = telemetry::get_trace_id();
    Span::current().record("trace_id", field::display(&trace_id));
    let _timer = ctx.metrics.count_and_measure("branch");
    ctx.diagnostics.write().await.last_event = Utc::now();

    let ns = neon_branch.namespace().unwrap(); // neon_branch is namespace scoped
    let branch_client: Api<NeonBranch> = Api::namespaced(ctx.client.clone(), &ns);

    info!(
        "Reconciling neon_branchument \"{}\" in {}",
        neon_branch.name_any(),
        ns
    );
    finalizer(
        &branch_client,
        NEON_BRANCH_FINALIZER,
        neon_branch.clone(),
        |event| async {
            match event {
                Finalizer::Apply(neon_branch) => neon_branch.reconcile(ctx.clone()).await,
                Finalizer::Cleanup(neon_branch) => {
                    neon_branch
                        .cleanup(ctx.clone(), &neon_branch.object_ref(&()))
                        .await
                }
            }
        },
    )
    .await
    .map_err(|e| {
        error!("Failed to reconcile neon_branch: {}", e);
        errors::Error::StdError(errors::StdError::FinalizerError(Box::new(e)))
    })
}

/// Diagnostics to be exposed by the web server
#[derive(Clone, Serialize)]
pub struct Diagnostics {
    #[serde(deserialize_with = "from_ts")]
    pub last_event: DateTime<Utc>,
    #[serde(skip)]
    pub reporter: Reporter,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            last_event: Utc::now(),
            reporter: "doc-controller".into(),
        }
    }
}
impl Diagnostics {
    fn recorder(&self, client: Client) -> Recorder {
        Recorder::new(client, self.reporter.clone())
    }
}

fn error_policy(neon_branch: Arc<NeonBranch>, error: &errors::Error, ctx: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    ctx.metrics.reconcile_branch_failure(&neon_branch, error);
    Action::requeue(Duration::from_secs(5 * 60))
}

/// Initialize the controller and shared state (given the crd is installed)
pub async fn run(state: State) {
    let client = Client::try_default().await.expect("failed to create kube Client");

    let branch_client = Api::<NeonBranch>::all(client.clone());
    if let Err(e) = branch_client.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
        std::process::exit(1);
    }

    Controller::new(branch_client, Config::default().any_semantic())
        .owns(Api::<Service>::all(client.clone()), watcher::Config::default())
        .owns(Api::<Deployment>::all(client.clone()), watcher::Config::default())
        .run(reconcile, error_policy, state.to_context(client))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
